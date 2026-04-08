#[cfg(target_arch = "wasm32")]
mod wasm {
    use std::{
        cell::RefCell,
        rc::Rc,
        sync::Arc,
        time::{Duration, Instant},
    };

    use midi_file::midly::MidiMessage;
    use neothesia_core::{
        Color, Gpu, TransformUniform, Uniform,
        config::Config,
        piano_layout,
        render::{KeyboardRenderer, QuadRenderer, QuadRendererFactory, WaterfallRenderer},
    };
    use wasm_bindgen::JsCast;
    use wgpu_jumpstart::Surface;
    use winit::{
        application::ApplicationHandler,
        event::WindowEvent,
        event_loop::{ActiveEventLoop, EventLoop},
        platform::web::{EventLoopExtWebSys, WindowAttributesExtWebSys},
        window::Window,
    };

    struct WebPiano {
        window: Arc<Window>,
        gpu: Gpu,
        surface: Surface,
        transform: Uniform<TransformUniform>,
        config: Config,
        playback: midi_file::PlaybackState,
        keyboard: KeyboardRenderer,
        waterfall: WaterfallRenderer,
        quad_renderer: QuadRenderer,
        frame_timestamp: Instant,
    }

    impl WebPiano {
        async fn new(
            window: Arc<Window>,
            canvas: web_sys::HtmlCanvasElement,
        ) -> Result<Self, String> {
            let size = window.inner_size();
            let (gpu, surface) = Gpu::for_window(
                || wgpu::SurfaceTarget::Canvas(canvas.clone()),
                size.width,
                size.height,
            )
            .await
            .map_err(|err| format!("failed to initialize WebGPU: {err}"))?;

            let transform = Uniform::new(
                &gpu.device,
                TransformUniform::default(),
                wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
            );

            let config = Config::new();
            let midi =
                midi_file::MidiFile::from_bytes("test.mid", include_bytes!("../../test.mid"))
                    .map_err(|err| format!("embedded MIDI failed to parse: {err}"))?;
            let playback =
                midi_file::PlaybackState::new(Duration::from_secs(1), midi.tracks.clone());

            let layout = keyboard_layout(
                size.width as f32 / window.scale_factor() as f32,
                size.height as f32 / window.scale_factor() as f32,
                piano_layout::KeyboardRange::new(config.piano_range()),
            );
            let mut keyboard = KeyboardRenderer::new(layout.clone());
            keyboard
                .position_on_bottom_of_parent(size.height as f32 / window.scale_factor() as f32);

            let waterfall =
                WaterfallRenderer::new(&gpu, &midi.tracks, &[], &config, &transform, layout);

            let quad_factory = QuadRendererFactory::new(&gpu, &transform);
            let quad_renderer = quad_factory.new_renderer();

            let mut app = Self {
                window,
                gpu,
                surface,
                transform,
                config,
                playback,
                keyboard,
                waterfall,
                quad_renderer,
                frame_timestamp: Instant::now(),
            };
            app.resize();
            app.gpu.submit();
            Ok(app)
        }

        fn resize(&mut self) {
            let physical_size = self.window.inner_size();
            if physical_size.width == 0 || physical_size.height == 0 {
                return;
            }

            self.surface.resize_swap_chain(
                &self.gpu.device,
                physical_size.width,
                physical_size.height,
            );

            let scale_factor = self.window.scale_factor() as f32;
            let logical_size = physical_size.to_logical::<f32>(scale_factor as f64);

            self.transform.data.update(
                physical_size.width as f32,
                physical_size.height as f32,
                scale_factor,
            );
            self.transform.update(&self.gpu.queue);

            let layout = keyboard_layout(
                logical_size.width,
                logical_size.height,
                piano_layout::KeyboardRange::new(self.config.piano_range()),
            );
            self.keyboard.set_layout(layout.clone());
            self.keyboard
                .position_on_bottom_of_parent(logical_size.height);
            self.waterfall.resize(&self.config, layout);
        }

        fn update(&mut self, delta: Duration) {
            if self.playback.is_finished() {
                self.playback.reset();
                self.keyboard.reset_notes();
            }

            let events = self.playback.update(delta);
            let range_start = self.keyboard.range().start() as usize;

            for event in events {
                let (is_on, key) = match event.message {
                    MidiMessage::NoteOn { key, vel } if vel.as_int() > 0 => (true, key.as_int()),
                    MidiMessage::NoteOn { key, .. } | MidiMessage::NoteOff { key, .. } => {
                        (false, key.as_int())
                    }
                    _ => continue,
                };

                if !self.keyboard.range().contains(key) || event.channel == 9 {
                    continue;
                }

                let id = key as usize - range_start;
                let color_schema = self.config.color_schema();
                let color = &color_schema[event.track_color_id % color_schema.len()];
                let key_state = &mut self.keyboard.key_states_mut()[id];

                if is_on {
                    key_state.pressed_by_file_on(color);
                } else {
                    key_state.pressed_by_file_off();
                }
                self.keyboard.invalidate_cache();
            }

            self.waterfall
                .update(self.playback.time().as_secs_f32() - self.playback.leed_in().as_secs_f32());
            self.quad_renderer.clear();
            self.keyboard.update_quads_only(&mut self.quad_renderer);
            self.quad_renderer.prepare();
        }

        fn render(&mut self) {
            let frame = match self.surface.get_current_texture() {
                Ok(frame) => frame,
                Err(err) => {
                    log::warn!("failed to acquire web surface texture: {err:?}");
                    return;
                }
            };

            let view = frame
                .texture
                .create_view(&wgpu::TextureViewDescriptor::default());
            let bg_color = Color::from(self.config.background_color()).into_linear_wgpu_color();

            {
                let rpass = self
                    .gpu
                    .encoder
                    .begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("PianoPro Web Pass"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &view,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(bg_color),
                                store: wgpu::StoreOp::Store,
                            },
                            depth_slice: None,
                        })],
                        depth_stencil_attachment: None,
                        timestamp_writes: None,
                        occlusion_query_set: None,
                        multiview_mask: None,
                    });

                let mut rpass = wgpu_jumpstart::RenderPass::new(rpass, frame.texture.size());
                self.waterfall.render(&mut rpass);
                self.quad_renderer.render(&mut rpass);
            }

            self.gpu.submit();
            self.window.pre_present_notify();
            frame.present();
        }

        fn redraw(&mut self) {
            let now = Instant::now();
            let delta = now.duration_since(self.frame_timestamp);
            self.frame_timestamp = now;

            self.update(delta);
            self.render();
        }
    }

    struct WebBootstrap {
        canvas: web_sys::HtmlCanvasElement,
        app: Rc<RefCell<Option<WebPiano>>>,
        initializing: bool,
    }

    impl ApplicationHandler for WebBootstrap {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            if self.initializing || self.app.borrow().is_some() {
                return;
            }
            self.initializing = true;

            let attributes = Window::default_attributes()
                .with_title("PianoPro")
                .with_canvas(Some(self.canvas.clone()));
            let window = match event_loop.create_window(attributes) {
                Ok(window) => Arc::new(window),
                Err(err) => {
                    log_browser_error(&format!("failed to create winit window: {err}"));
                    return;
                }
            };
            let app_cell = self.app.clone();
            let canvas = self.canvas.clone();
            wasm_bindgen_futures::spawn_local(async move {
                match WebPiano::new(window, canvas).await {
                    Ok(app) => {
                        app.window.request_redraw();
                        app_cell.borrow_mut().replace(app);
                    }
                    Err(err) => log_browser_error(&err),
                }
            });
        }

        fn window_event(
            &mut self,
            event_loop: &ActiveEventLoop,
            _window_id: winit::window::WindowId,
            event: WindowEvent,
        ) {
            let mut app = self.app.borrow_mut();
            let Some(app) = app.as_mut() else {
                return;
            };

            match event {
                WindowEvent::Resized(size) if size.width > 0 && size.height > 0 => {
                    app.resize();
                    app.window.request_redraw();
                }
                WindowEvent::ScaleFactorChanged { .. } => {
                    app.resize();
                    app.window.request_redraw();
                }
                WindowEvent::RedrawRequested => app.redraw(),
                WindowEvent::CloseRequested => event_loop.exit(),
                _ => {}
            }
        }

        fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
            let app = self.app.borrow();
            if let Some(app) = app.as_ref() {
                app.window.request_redraw();
            }
        }
    }

    fn keyboard_layout(
        width: f32,
        height: f32,
        range: piano_layout::KeyboardRange,
    ) -> piano_layout::KeyboardLayout {
        let neutral_width = width / range.white_count() as f32;
        let neutral_height = height * 0.2;
        piano_layout::KeyboardLayout::from_range(
            piano_layout::Sizing::new(neutral_width, neutral_height),
            range,
        )
    }

    fn log_browser_error(message: &str) {
        web_sys::console::error_1(&message.into());
    }

    fn set_panic_hook() {
        std::panic::set_hook(Box::new(|info| {
            log_browser_error(&format!("PianoPro web panic: {info}"));
        }));
    }

    pub fn run() {
        set_panic_hook();

        let window = web_sys::window().expect("window should exist");
        let document = window.document().expect("document should exist");
        let canvas = document
            .get_element_by_id("pianopro-canvas")
            .expect("pianopro canvas should exist")
            .dyn_into::<web_sys::HtmlCanvasElement>()
            .expect("pianopro canvas should be a canvas element");

        let event_loop = EventLoop::new().unwrap();
        event_loop.spawn_app(WebBootstrap {
            canvas,
            app: Rc::new(RefCell::new(None)),
            initializing: false,
        });
    }
}

#[cfg(target_arch = "wasm32")]
fn main() {
    wasm::run();
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    eprintln!("neothesia-web is intended to run with the wasm32-unknown-unknown target.");
}
