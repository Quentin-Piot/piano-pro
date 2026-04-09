#[cfg(target_arch = "wasm32")]
mod wasm {
    use std::{cell::RefCell, rc::Rc, sync::Arc, time::Duration};

    use midi_file::midly::MidiMessage;
    use neothesia::{
        Context, NeothesiaEvent, Scene, Song,
        output_manager::web_backend::{WebAudioCmd, WebAudioQueue, WebOutputSender},
        scene::{freeplay, menu_scene, playing_scene},
        utils::window::WindowState,
    };
    use wasm_bindgen::JsCast;
    use wgpu_jumpstart::{Gpu, Surface};
    use winit::{
        application::ApplicationHandler,
        event::{ElementState, MouseButton, TouchPhase, WindowEvent},
        event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy},
        platform::web::{EventLoopExtWebSys, WindowAttributesExtWebSys},
        window::Window,
    };

    // ── Audio ──────────────────────────────────────────────────────────────────

    const AUDIO_CHUNK_SIZE: usize = 2048;
    const AUDIO_LOOKAHEAD: f64 = 0.1;

    struct OxiAudioEngine {
        context: web_sys::AudioContext,
        synth: oxisynth::Synth,
        sample_rate: f32,
        next_schedule_time: f64,
    }

    impl OxiAudioEngine {
        fn new() -> Result<Self, String> {
            let context = web_sys::AudioContext::new()
                .map_err(|_| String::from("failed to create AudioContext"))?;

            let sample_rate = context.sample_rate();

            let font = oxisynth::SoundFont::load(&mut std::io::Cursor::new(include_bytes!(
                "../../default.sf2"
            )))
            .map_err(|e| format!("failed to load soundfont: {e}"))?;

            let mut synth = oxisynth::Synth::new(oxisynth::SynthDescriptor {
                sample_rate,
                gain: 0.5,
                ..Default::default()
            })
            .map_err(|e| format!("failed to create synth: {e}"))?;

            synth.add_font(font, true);

            Ok(Self {
                context,
                synth,
                sample_rate,
                next_schedule_time: 0.0,
            })
        }

        fn activate(&mut self) {
            let _ = self.context.resume();
            let current_time = self.context.current_time();
            if self.next_schedule_time < current_time {
                self.next_schedule_time = current_time;
            }
        }

        fn stop_all(&mut self) {
            for channel in 0..16u8 {
                self.synth
                    .send_event(oxisynth::MidiEvent::AllNotesOff { channel })
                    .ok();
                self.synth
                    .send_event(oxisynth::MidiEvent::AllSoundOff { channel })
                    .ok();
            }
        }

        fn handle_midi_event(&mut self, channel: u8, message: MidiMessage) {
            let event = match message {
                MidiMessage::NoteOn { key, vel } if vel.as_int() > 0 => {
                    oxisynth::MidiEvent::NoteOn {
                        channel,
                        key: key.as_int(),
                        vel: vel.as_int(),
                    }
                }
                MidiMessage::NoteOn { key, .. } | MidiMessage::NoteOff { key, .. } => {
                    oxisynth::MidiEvent::NoteOff {
                        channel,
                        key: key.as_int(),
                    }
                }
                _ => return,
            };
            self.synth.send_event(event).ok();
        }

        fn pump_audio(&mut self) {
            let current_time = self.context.current_time();

            if self.next_schedule_time < current_time {
                self.next_schedule_time = current_time;
            }

            let target_time = current_time + AUDIO_LOOKAHEAD;
            let chunk_duration = AUDIO_CHUNK_SIZE as f64 / self.sample_rate as f64;

            while self.next_schedule_time < target_time {
                let mut left = vec![0f32; AUDIO_CHUNK_SIZE];
                let mut right = vec![0f32; AUDIO_CHUNK_SIZE];
                for i in 0..AUDIO_CHUNK_SIZE {
                    let (l, r) = self.synth.read_next();
                    left[i] = l;
                    right[i] = r;
                }

                let Ok(buffer) =
                    self.context
                        .create_buffer(2, AUDIO_CHUNK_SIZE as u32, self.sample_rate)
                else {
                    break;
                };
                if buffer.copy_to_channel(&left, 0).is_err()
                    || buffer.copy_to_channel(&right, 1).is_err()
                {
                    break;
                }

                let Ok(source) = self.context.create_buffer_source() else {
                    break;
                };
                source.set_buffer(Some(&buffer));
                if source
                    .connect_with_audio_node(&self.context.destination())
                    .is_err()
                {
                    break;
                }
                if source.start_with_when(self.next_schedule_time).is_err() {
                    break;
                }

                self.next_schedule_time += chunk_duration;
            }
        }
    }

    // ── WebApp ─────────────────────────────────────────────────────────────────

    struct WebApp {
        context: Context,
        game_scene: Box<dyn Scene>,
        // Dropped last (wgpu internal ref-counting)
        surface: Surface,
        audio: Option<OxiAudioEngine>,
        audio_queue: WebAudioQueue,
    }

    impl WebApp {
        async fn new(
            window: Arc<Window>,
            canvas: web_sys::HtmlCanvasElement,
            proxy: EventLoopProxy<NeothesiaEvent>,
        ) -> Result<Self, String> {
            let size = initial_surface_size(&window, &canvas);
            canvas.set_width(size.width);
            canvas.set_height(size.height);

            let (gpu, surface) = Gpu::for_window(
                || wgpu::SurfaceTarget::Canvas(canvas.clone()),
                size.width,
                size.height,
            )
            .await
            .map_err(|err| format!("failed to initialize WebGPU: {err}"))?;

            let window_state = WindowState::new(&window);
            let mut context = Context::new(window, window_state, proxy, gpu);
            context.resize();
            context.gpu.submit();

            // Wire up web audio output
            let (sender, audio_queue) = WebOutputSender::new();
            context.output_manager.connect_web(sender);

            // Embed demo song as the initial preloaded song
            let demo_song = {
                let bytes = include_bytes!("../../test.mid");
                midi_file::MidiFile::from_bytes("test.mid", bytes)
                    .ok()
                    .map(|m| Song::with_display_name(m, "Demo Song".into()))
            };

            let game_scene = menu_scene::MenuScene::new(&mut context, demo_song);

            Ok(Self {
                context,
                game_scene: Box::new(game_scene),
                surface,
                audio: None,
                audio_queue,
            })
        }

        fn ensure_audio_started(&mut self) -> Result<(), String> {
            if self.audio.is_none() {
                self.audio = Some(OxiAudioEngine::new()?);
            }
            if let Some(audio) = self.audio.as_mut() {
                audio.activate();
            }
            Ok(())
        }

        fn drain_audio_queue(&mut self) {
            let cmds: Vec<_> = self.audio_queue.borrow_mut().drain(..).collect();
            if let Some(audio) = self.audio.as_mut() {
                for cmd in cmds {
                    match cmd {
                        WebAudioCmd::Midi { channel, message } => {
                            audio.handle_midi_event(channel, message)
                        }
                        WebAudioCmd::StopAll => audio.stop_all(),
                    }
                }
                audio.pump_audio();
            }
        }

        fn resize(&mut self) {
            let physical_size = self.context.window.inner_size();
            if physical_size.width == 0 || physical_size.height == 0 {
                return;
            }
            self.surface.resize_swap_chain(
                &self.context.gpu.device,
                physical_size.width,
                physical_size.height,
            );
            self.context.resize();
        }

        fn update(&mut self, delta: Duration) {
            self.game_scene.update(&mut self.context, delta);
        }

        fn render(&mut self) {
            let frame = match self.surface.get_current_texture() {
                Ok(f) => f,
                Err(err) => {
                    log::warn!("failed to acquire web surface texture: {err:?}");
                    return;
                }
            };

            let view = frame
                .texture
                .create_view(&wgpu::TextureViewDescriptor::default());

            let bg_color =
                wgpu_jumpstart::Color::from(self.context.config.background_color())
                    .into_linear_wgpu_color();

            {
                let rpass =
                    self.context
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
                self.game_scene.render(&mut rpass);
            }

            self.context.gpu.submit();
            self.context.window.pre_present_notify();
            frame.present();
            self.context.text_renderer_factory.end_frame();
        }
    }

    // ── Bootstrap ──────────────────────────────────────────────────────────────

    struct WebBootstrap {
        canvas: web_sys::HtmlCanvasElement,
        proxy: EventLoopProxy<NeothesiaEvent>,
        app: Rc<RefCell<Option<WebApp>>>,
        initializing: bool,
    }

    impl ApplicationHandler<NeothesiaEvent> for WebBootstrap {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            if self.initializing || self.app.borrow().is_some() {
                return;
            }
            self.initializing = true;

            let attributes = winit::window::Window::default_attributes()
                .with_title("PianoPro")
                .with_canvas(Some(self.canvas.clone()));

            let window = match event_loop.create_window(attributes) {
                Ok(w) => Arc::new(w),
                Err(err) => {
                    log::error!("failed to create web window: {err}");
                    self.initializing = false;
                    return;
                }
            };

            let app_cell = self.app.clone();
            let proxy = self.proxy.clone();
            let canvas = self.canvas.clone();

            wasm_bindgen_futures::spawn_local(async move {
                match WebApp::new(window, canvas, proxy).await {
                    Ok(app) => {
                        app.context.window.request_redraw();
                        *app_cell.borrow_mut() = Some(app);
                    }
                    Err(err) => log::error!("PianoPro web init failed: {err}"),
                }
            });
        }

        fn user_event(&mut self, event_loop: &ActiveEventLoop, event: NeothesiaEvent) {
            let mut app_ref = self.app.borrow_mut();
            let Some(app) = app_ref.as_mut() else {
                return;
            };

            match event {
                NeothesiaEvent::Play(song) => {
                    if let Err(err) = app.ensure_audio_started() {
                        log::error!("audio init failed: {err}");
                    }
                    let scene = playing_scene::PlayingScene::new(&mut app.context, song);
                    app.game_scene = Box::new(scene);
                    app.context.window.request_redraw();
                }
                NeothesiaEvent::FreePlay(song) => {
                    if let Err(err) = app.ensure_audio_started() {
                        log::error!("audio init failed: {err}");
                    }
                    let scene = freeplay::FreeplayScene::new(&mut app.context, song);
                    app.game_scene = Box::new(scene);
                    app.context.window.request_redraw();
                }
                NeothesiaEvent::MainMenu(song) => {
                    let scene = menu_scene::MenuScene::new(&mut app.context, song);
                    app.game_scene = Box::new(scene);
                    app.context.window.request_redraw();
                }
                NeothesiaEvent::MidiInput { channel, message } => {
                    app.game_scene
                        .midi_event(&mut app.context, channel, &message);
                }
                NeothesiaEvent::Exit => {
                    drop(app_ref);
                    event_loop.exit();
                }
            }
        }

        fn window_event(
            &mut self,
            event_loop: &ActiveEventLoop,
            window_id: winit::window::WindowId,
            event: WindowEvent,
        ) {
            // Translate touch events to mouse events (same as desktop)
            if let WindowEvent::Touch(touch) = &event {
                let touch = *touch;
                match touch.phase {
                    TouchPhase::Started => {
                        self.window_event(
                            event_loop,
                            window_id,
                            WindowEvent::CursorMoved {
                                device_id: touch.device_id,
                                position: touch.location,
                            },
                        );
                        self.window_event(
                            event_loop,
                            window_id,
                            WindowEvent::MouseInput {
                                device_id: touch.device_id,
                                state: ElementState::Pressed,
                                button: MouseButton::Left,
                            },
                        );
                    }
                    TouchPhase::Ended | TouchPhase::Cancelled => {
                        self.window_event(
                            event_loop,
                            window_id,
                            WindowEvent::MouseInput {
                                device_id: touch.device_id,
                                state: ElementState::Released,
                                button: MouseButton::Left,
                            },
                        );
                    }
                    TouchPhase::Moved => {
                        self.window_event(
                            event_loop,
                            window_id,
                            WindowEvent::CursorMoved {
                                device_id: touch.device_id,
                                position: touch.location,
                            },
                        );
                    }
                }
                return;
            }

            let mut app_ref = self.app.borrow_mut();
            let Some(app) = app_ref.as_mut() else {
                if let WindowEvent::CloseRequested = event {
                    event_loop.exit();
                }
                return;
            };

            app.context.window_state.window_event(&event);

            match event {
                WindowEvent::Resized(size) if size.width > 0 && size.height > 0 => {
                    app.resize();
                    app.context.window.request_redraw();
                }
                WindowEvent::ScaleFactorChanged { .. } => {
                    app.context.resize();
                    app.context.window.request_redraw();
                }
                WindowEvent::RedrawRequested => {
                    let delta = app.context.frame_timestamp.elapsed();
                    app.context.frame_timestamp = web_time::Instant::now();
                    app.drain_audio_queue();
                    app.update(delta);
                    app.render();
                }
                WindowEvent::CloseRequested => event_loop.exit(),
                _ => {
                    app.game_scene.window_event(&mut app.context, &event);
                }
            }
        }

        fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
            if let Some(app) = self.app.borrow().as_ref() {
                app.context.window.request_redraw();
            }
        }
    }

    // ── Helpers ────────────────────────────────────────────────────────────────

    fn initial_surface_size(
        window: &Window,
        canvas: &web_sys::HtmlCanvasElement,
    ) -> winit::dpi::PhysicalSize<u32> {
        let size = window.inner_size();
        if size.width > 0 && size.height > 0 {
            return size;
        }

        let scale_factor = web_sys::window()
            .map(|w| w.device_pixel_ratio())
            .unwrap_or(1.0);
        let width = (f64::from(canvas.client_width()).max(1.0) * scale_factor).round() as u32;
        let height = (f64::from(canvas.client_height()).max(1.0) * scale_factor).round() as u32;

        winit::dpi::PhysicalSize::new(width.max(1), height.max(1))
    }

    fn set_panic_hook() {
        std::panic::set_hook(Box::new(|info| {
            web_sys::console::error_1(&format!("PianoPro web panic: {info}").into());
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

        let event_loop: EventLoop<NeothesiaEvent> =
            EventLoop::with_user_event().build().unwrap();
        let proxy = event_loop.create_proxy();

        let bootstrap = WebBootstrap {
            canvas,
            proxy,
            app: Rc::new(RefCell::new(None)),
            initializing: false,
        };

        event_loop.spawn_app(bootstrap);
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
