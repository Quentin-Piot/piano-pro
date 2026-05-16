#[cfg(target_os = "android")]
mod android_file_picker;

#[cfg(target_os = "android")]
mod android {
    use std::{sync::Arc, time::Duration};

    use android_activity::AndroidApp;
    use neothesia::{
        Context, NeothesiaEvent, Scene, Song,
        output_manager::OutputDescriptor,
        scene::{freeplay, menu_scene, playing_scene},
        utils::window::WindowState,
    };
    use wgpu_jumpstart::{Gpu, Surface};
    use winit::{
        application::ApplicationHandler,
        dpi::PhysicalPosition,
        event::{ElementState, MouseButton, MouseScrollDelta, TouchPhase, WindowEvent},
        event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy},
        platform::android::EventLoopBuilderExtAndroid,
        window::Window,
    };

    struct Live {
        ctx: Context,
        surface: Surface,
        scene: Box<dyn Scene>,
    }

    impl Live {
        fn new(mut ctx: Context, surface: Surface) -> Self {
            let song = {
                let bytes = include_bytes!("../../test.mid");
                midi_file::MidiFile::from_bytes("test.mid", bytes)
                    .ok()
                    .map(|m| Song::with_display_name(m, "Demo Song".into()))
            };
            let scene: Box<dyn Scene> = Box::new(menu_scene::MenuScene::new(&mut ctx, song));
            Self {
                ctx,
                surface,
                scene,
            }
        }

        fn update(&mut self, delta: Duration) {
            self.scene.update(&mut self.ctx, delta);
        }

        fn render(&mut self) {
            let frame = loop {
                match self.surface.get_current_texture() {
                    Ok(f) => break f,
                    Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                        let size = self.ctx.window_state.physical_size;
                        self.surface.resize_swap_chain(
                            &self.ctx.gpu.device,
                            size.width,
                            size.height,
                        );
                    }
                    Err(wgpu::SurfaceError::Timeout) => return,
                    Err(err) => {
                        log::error!("Fatal surface error: {err:?}");
                        return;
                    }
                }
            };

            let view = frame
                .texture
                .create_view(&wgpu::TextureViewDescriptor::default());

            {
                let bg = self.ctx.config.background_color();
                let bg = wgpu_jumpstart::Color::from(bg).into_linear_wgpu_color();
                let rpass = self
                    .ctx
                    .gpu
                    .encoder
                    .begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("Main PianoPro Pass"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &view,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(bg),
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
                self.scene.render(&mut rpass);
            }

            self.ctx.gpu.submit();
            self.ctx.window.pre_present_notify();
            frame.present();
            self.ctx.text_renderer_factory.end_frame();
        }

        fn on_window_event(&mut self, event: &WindowEvent) {
            self.ctx.window_state.window_event(event);
            match event {
                WindowEvent::Resized(ps) if ps.width > 0 && ps.height > 0 => {
                    self.surface
                        .resize_swap_chain(&self.ctx.gpu.device, ps.width, ps.height);
                    self.ctx.resize();
                    self.ctx.window.request_redraw();
                }
                WindowEvent::ScaleFactorChanged { .. } => {
                    self.ctx.resize();
                }
                WindowEvent::RedrawRequested => {
                    let delta = self.ctx.frame_timestamp.elapsed();
                    self.ctx.frame_timestamp = web_time::Instant::now();
                    self.update(delta);
                    self.render();
                }
                _ => {
                    self.scene.window_event(&mut self.ctx, event);
                }
            }
        }
    }

    struct App {
        proxy: EventLoopProxy<NeothesiaEvent>,
        live: Option<Live>,
        last_touch_y: Option<f64>,
        touch_consumed: bool,
    }

    impl App {
        fn new(proxy: EventLoopProxy<NeothesiaEvent>) -> Self {
            Self {
                proxy,
                live: None,
                last_touch_y: None,
                touch_consumed: false,
            }
        }

        fn connect_synth(ctx: &mut Context) {
            ctx.output_manager.connect(OutputDescriptor::Synth(
                neothesia_core::utils::resources::default_sf2(),
            ));
        }
    }

    impl ApplicationHandler<NeothesiaEvent> for App {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            let window = Arc::new(
                event_loop
                    .create_window(Window::default_attributes().with_title("PianoPro"))
                    .expect("window"),
            );

            let window_state = WindowState::new(&window);
            let size = window.inner_size();

            let (gpu, surface) = pollster::block_on(Gpu::for_window(
                || window.clone().into(),
                size.width,
                size.height,
            ))
            .expect("GPU init");

            let mut ctx = Context::new(window, window_state, self.proxy.clone(), gpu);
            ctx.resize();
            ctx.gpu.submit();
            Self::connect_synth(&mut ctx);

            self.live = Some(Live::new(ctx, surface));
        }

        fn suspended(&mut self, _: &ActiveEventLoop) {
            self.live.take();
        }

        fn user_event(&mut self, event_loop: &ActiveEventLoop, event: NeothesiaEvent) {
            let Some(live) = &mut self.live else {
                return;
            };
            match event {
                NeothesiaEvent::Play(song) => {
                    live.scene = Box::new(playing_scene::PlayingScene::new(&mut live.ctx, song));
                }
                NeothesiaEvent::FreePlay(song) => {
                    live.scene = Box::new(freeplay::FreeplayScene::new(&mut live.ctx, song));
                }
                NeothesiaEvent::MainMenu(song) => {
                    live.scene = Box::new(menu_scene::MenuScene::new(&mut live.ctx, song));
                }
                NeothesiaEvent::MidiInput { channel, message } => {
                    live.scene.midi_event(&mut live.ctx, channel, &message);
                }
                NeothesiaEvent::Exit => event_loop.exit(),
            }
        }

        fn window_event(
            &mut self,
            _event_loop: &ActiveEventLoop,
            _window_id: winit::window::WindowId,
            event: WindowEvent,
        ) {
            let Some(live) = &mut self.live else {
                return;
            };

            if let WindowEvent::Touch(touch) = &event {
                let touch = *touch;
                let scale = live.ctx.window_state.scale_factor;
                let logical_pos = winit::dpi::LogicalPosition::new(
                    (touch.location.x / scale) as f32,
                    (touch.location.y / scale) as f32,
                );

                match touch.phase {
                    TouchPhase::Started => {
                        self.touch_consumed = false;
                        self.last_touch_y = Some(touch.location.y);
                        live.on_window_event(&WindowEvent::CursorMoved {
                            device_id: touch.device_id,
                            position: touch.location,
                        });
                        live.on_window_event(&WindowEvent::MouseInput {
                            device_id: touch.device_id,
                            state: ElementState::Pressed,
                            button: MouseButton::Left,
                        });
                        live.scene.touch_event(&mut live.ctx, TouchPhase::Started, logical_pos);
                    }
                    TouchPhase::Moved => {
                        let dy = self
                            .last_touch_y
                            .replace(touch.location.y)
                            .map(|prev| touch.location.y - prev)
                            .unwrap_or(0.0);

                        let seeking = live
                            .scene
                            .touch_event(&mut live.ctx, TouchPhase::Moved, logical_pos);

                        if seeking && !self.touch_consumed {
                            live.on_window_event(&WindowEvent::CursorMoved {
                                device_id: touch.device_id,
                                position: PhysicalPosition::new(-1.0_f64, -1.0_f64),
                            });
                            live.on_window_event(&WindowEvent::MouseInput {
                                device_id: touch.device_id,
                                state: ElementState::Released,
                                button: MouseButton::Left,
                            });
                            self.touch_consumed = true;
                        } else if !self.touch_consumed {
                            live.on_window_event(&WindowEvent::CursorMoved {
                                device_id: touch.device_id,
                                position: touch.location,
                            });
                            if dy.abs() > 0.5 {
                                live.on_window_event(&WindowEvent::MouseWheel {
                                    device_id: touch.device_id,
                                    delta: MouseScrollDelta::PixelDelta(PhysicalPosition::new(
                                        0.0, dy,
                                    )),
                                    phase: TouchPhase::Moved,
                                });
                            }
                        }
                    }
                    TouchPhase::Ended | TouchPhase::Cancelled => {
                        self.last_touch_y = None;
                        live.scene
                            .touch_event(&mut live.ctx, touch.phase, logical_pos);
                        if !self.touch_consumed {
                            live.on_window_event(&WindowEvent::MouseInput {
                                device_id: touch.device_id,
                                state: ElementState::Released,
                                button: MouseButton::Left,
                            });
                        }
                        self.touch_consumed = false;
                    }
                }
            } else {
                live.on_window_event(&event);
            }
        }

        fn about_to_wait(&mut self, _: &ActiveEventLoop) {
            if let Some(live) = &self.live {
                live.ctx.window.request_redraw();
            }
        }
    }

    fn init_android_storage(app: &AndroidApp) {
        let Some(data_dir) = app.internal_data_path() else {
            return;
        };
        neothesia_core::utils::resources::set_android_data_path(data_dir.clone());

        // Copy SF2 only if missing or corrupted (size mismatch)
        let sf2_path = data_dir.join("default.sf2");
        let bundled_sf2 = include_bytes!("../../default.sf2");
        
        if !sf2_path.exists() {
            // First time: copy the file
            if let Err(e) = std::fs::write(&sf2_path, bundled_sf2) {
                log::error!("Failed to write bundled SF2: {e}");
            }
        } else if let Ok(metadata) = std::fs::metadata(&sf2_path) {
            // Check if file matches expected size
            if metadata.len() != bundled_sf2.len() as u64 {
                // File is corrupted or wrong version, overwrite
                if let Err(e) = std::fs::write(&sf2_path, bundled_sf2) {
                    log::error!("Failed to update bundled SF2: {e}");
                }
            }
        } else {
            // File exists but can't read metadata, try to overwrite
            if let Err(e) = std::fs::write(&sf2_path, bundled_sf2) {
                log::error!("Failed to write bundled SF2: {e}");
            }
        }

        if let Some(ext_dir) = app.external_data_path() {
            neothesia_core::utils::resources::set_android_external_path(ext_dir.clone());
            let midi_dir = ext_dir.join("midi");
            if let Err(e) = std::fs::create_dir_all(&midi_dir) {
                log::error!("Failed to create external midi dir: {e}");
            }
        } else {
            let midi_dir = data_dir.join("midi");
            if let Err(e) = std::fs::create_dir_all(&midi_dir) {
                log::error!("Failed to create midi library dir: {e}");
            }
        }
    }

    #[unsafe(no_mangle)]
    fn android_main(app: AndroidApp) {
        android_logger::init_once(
            android_logger::Config::default().with_max_level(log::LevelFilter::Info),
        );

        init_android_storage(&app);
        crate::android_file_picker::init(app.clone());

        let event_loop = EventLoop::with_user_event()
            .with_android_app(app)
            .build()
            .expect("event loop");

        let proxy = event_loop.create_proxy();
        event_loop
            .run_app(&mut App::new(proxy))
            .expect("event loop run");
    }
}
