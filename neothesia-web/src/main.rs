#[cfg(target_arch = "wasm32")]
mod wasm {
    use std::{cell::RefCell, rc::Rc, sync::Arc, time::Duration};

    use midi_file::midly::MidiMessage;
    use neothesia_core::{
        Color, Gpu, TransformUniform, Uniform,
        config::Config,
        piano_layout,
        render::{KeyboardRenderer, QuadRenderer, QuadRendererFactory, WaterfallRenderer},
    };
    use rfd::AsyncFileDialog;
    use serde::{Deserialize, Serialize};
    use wasm_bindgen::{JsCast, closure::Closure};
    use wgpu_jumpstart::Surface;
    use winit::{
        application::ApplicationHandler,
        dpi::PhysicalSize,
        event::WindowEvent,
        event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy},
        platform::web::{EventLoopExtWebSys, WindowAttributesExtWebSys},
        window::Window,
    };

    #[derive(Debug, Clone)]
    enum WebEvent {
        AppInitialized,
        AppInitFailed(String),
        PlayDemo,
        ImportStarted,
        ImportCanceled,
        SongImported(Result<WebSong, String>),
        SelectSong(usize),
        StartSelected,
        ReturnHome,
    }

    #[derive(Debug, Clone)]
    enum WebSongOrigin {
        Demo,
        Imported,
    }

    #[derive(Debug, Clone)]
    struct WebSong {
        display_name: String,
        file_name: String,
        bytes: Vec<u8>,
        origin: WebSongOrigin,
    }

    impl WebSong {
        fn demo() -> Self {
            let file_name = String::from("test.mid");
            let bytes = include_bytes!("../../test.mid").to_vec();
            let display_name = midi_file::extract_midi_metadata_from_bytes(&file_name, &bytes)
                .unwrap_or_else(|| String::from("Embedded Demo"));

            Self {
                display_name,
                file_name,
                bytes,
                origin: WebSongOrigin::Demo,
            }
        }

        fn imported(file_name: String, bytes: Vec<u8>) -> Self {
            let display_name = midi_file::extract_midi_metadata_from_bytes(&file_name, &bytes)
                .unwrap_or_else(|| {
                    std::path::Path::new(&file_name)
                        .file_stem()
                        .and_then(|stem| stem.to_str())
                        .unwrap_or("Untitled")
                        .to_string()
                });

            Self {
                display_name,
                file_name,
                bytes,
                origin: WebSongOrigin::Imported,
            }
        }

        fn origin_label(&self) -> &'static str {
            match self.origin {
                WebSongOrigin::Demo => "Demo",
                WebSongOrigin::Imported => "Imported this session",
            }
        }
    }

    impl PersistedWebSong {
        fn from_song(song: &WebSong) -> Option<Self> {
            matches!(song.origin, WebSongOrigin::Imported).then(|| Self {
                display_name: song.display_name.clone(),
                file_name: song.file_name.clone(),
                bytes: song.bytes.clone(),
            })
        }

        fn into_song(self) -> Result<WebSong, String> {
            midi_file::MidiFile::from_bytes(self.file_name.clone(), &self.bytes)
                .map_err(|err| format!("failed to restore stored MIDI '{}': {err}", self.file_name))
                .map(|_| WebSong {
                    display_name: self.display_name,
                    file_name: self.file_name,
                    bytes: self.bytes,
                    origin: WebSongOrigin::Imported,
                })
        }
    }

    #[derive(Debug, Serialize, Deserialize)]
    struct PersistedWebSong {
        display_name: String,
        file_name: String,
        bytes: Vec<u8>,
    }

    #[derive(Debug, Serialize, Deserialize)]
    struct PersistedWebLibrary {
        version: u8,
        songs: Vec<PersistedWebSong>,
        selected_song: Option<usize>,
    }

    struct WebPlayback {
        title: String,
        playback: midi_file::PlaybackState,
        keyboard: KeyboardRenderer,
        waterfall: WaterfallRenderer,
        quad_renderer: QuadRenderer,
    }

    impl WebPlayback {
        fn new(
            gpu: &Gpu,
            transform: &Uniform<TransformUniform>,
            config: &Config,
            window: &Window,
            physical_size: PhysicalSize<u32>,
            song: WebSong,
        ) -> Result<Self, String> {
            let midi = midi_file::MidiFile::from_bytes(song.file_name.clone(), &song.bytes)
                .map_err(|err| format!("failed to parse MIDI: {err}"))?;

            let scale_factor = window.scale_factor() as f32;
            let layout = keyboard_layout(
                physical_size.width as f32 / scale_factor,
                physical_size.height as f32 / scale_factor,
                piano_layout::KeyboardRange::new(config.piano_range()),
            );

            let mut keyboard = KeyboardRenderer::new(layout.clone());
            keyboard.position_on_bottom_of_parent(physical_size.height as f32 / scale_factor);

            let waterfall =
                WaterfallRenderer::new(gpu, &midi.tracks, &[], config, transform, layout);

            let quad_factory = QuadRendererFactory::new(gpu, transform);
            let mut quad_renderer = quad_factory.new_renderer();

            let playback =
                midi_file::PlaybackState::new(Duration::from_secs(1), midi.tracks.clone());

            keyboard.update_quads_only(&mut quad_renderer);
            quad_renderer.prepare();

            Ok(Self {
                title: song.display_name,
                playback,
                keyboard,
                waterfall,
                quad_renderer,
            })
        }

        fn resize(&mut self, config: &Config, logical_width: f32, logical_height: f32) {
            let layout = keyboard_layout(
                logical_width,
                logical_height,
                piano_layout::KeyboardRange::new(config.piano_range()),
            );
            self.keyboard.set_layout(layout.clone());
            self.keyboard.position_on_bottom_of_parent(logical_height);
            self.waterfall.resize(config, layout);
        }

        fn update_and_collect_events<'a>(
            &'a mut self,
            config: &Config,
            delta: Duration,
        ) -> Vec<(u8, MidiMessage)> {
            let mut audio_events = Vec::new();

            if self.playback.is_finished() {
                self.playback.reset();
                self.keyboard.reset_notes();
            }

            let events = self.playback.update(delta);
            let range_start = self.keyboard.range().start() as usize;

            for event in events {
                audio_events.push((event.channel, event.message));

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
                let color_schema = config.color_schema();
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

            audio_events
        }

        fn render<'pass>(&'pass mut self, rpass: &mut wgpu_jumpstart::RenderPass<'pass>) {
            self.waterfall.render(rpass);
            self.quad_renderer.render(rpass);
        }
    }

    const AUDIO_CHUNK_SIZE: usize = 2048;
    const AUDIO_LOOKAHEAD: f64 = 0.1; // seconds

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

            let font = oxisynth::SoundFont::load(&mut std::io::Cursor::new(
                include_bytes!("../../default.sf2"),
            ))
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

    struct WebApp {
        window: Arc<Window>,
        gpu: Gpu,
        surface: Surface,
        transform: Uniform<TransformUniform>,
        config: Config,
        playback: Option<WebPlayback>,
        audio: Option<OxiAudioEngine>,
        frame_timestamp: f64,
    }

    impl WebApp {
        async fn new(
            window: Arc<Window>,
            canvas: web_sys::HtmlCanvasElement,
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

            let transform = Uniform::new(
                &gpu.device,
                TransformUniform::default(),
                wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
            );

            let mut app = Self {
                window,
                gpu,
                surface,
                transform,
                config: Config::new(),
                playback: None,
                audio: None,
                frame_timestamp: now_seconds(),
            };
            app.resize();
            app.gpu.submit();
            Ok(app)
        }

        fn load_song(&mut self, song: WebSong) -> Result<String, String> {
            if let Some(audio) = self.audio.as_mut() {
                audio.stop_all();
            }
            let playback = WebPlayback::new(
                &self.gpu,
                &self.transform,
                &self.config,
                &self.window,
                self.window.inner_size(),
                song,
            )?;
            let title = playback.title.clone();
            self.playback = Some(playback);
            self.frame_timestamp = now_seconds();
            Ok(title)
        }

        fn unload_song(&mut self) {
            self.playback = None;
            if let Some(audio) = self.audio.as_mut() {
                audio.stop_all();
            }
            self.frame_timestamp = now_seconds();
        }

        fn has_playback(&self) -> bool {
            self.playback.is_some()
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

            if let Some(playback) = self.playback.as_mut() {
                playback.resize(&self.config, logical_size.width, logical_size.height);
            }
        }

        fn redraw(&mut self) {
            let now = now_seconds();
            let delta = Duration::from_secs_f64((now - self.frame_timestamp).max(0.0));
            self.frame_timestamp = now;

            if let Some(playback) = self.playback.as_mut() {
                let audio_events = playback.update_and_collect_events(&self.config, delta);
                if let Some(audio) = self.audio.as_mut() {
                    for (channel, message) in audio_events {
                        audio.handle_midi_event(channel, message);
                    }
                    audio.pump_audio();
                }
            }

            self.render();
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
                if let Some(playback) = self.playback.as_mut() {
                    playback.render(&mut rpass);
                }
            }

            self.gpu.submit();
            self.window.pre_present_notify();
            frame.present();
        }
    }

    struct WebShell {
        document: web_sys::Document,
        proxy: EventLoopProxy<WebEvent>,
        _root: web_sys::HtmlElement,
        home_panel: web_sys::HtmlElement,
        playback_bar: web_sys::HtmlElement,
        library_list: web_sys::HtmlElement,
        selected_title: web_sys::HtmlElement,
        selected_meta: web_sys::HtmlElement,
        status_text: web_sys::HtmlElement,
        error_text: web_sys::HtmlElement,
        playback_title: web_sys::HtmlElement,
        start_button: web_sys::HtmlElement,
        demo_button: web_sys::HtmlElement,
        import_button: web_sys::HtmlElement,
        back_button: web_sys::HtmlElement,
        static_listeners: Vec<Closure<dyn FnMut(web_sys::Event)>>,
        song_listeners: Vec<Closure<dyn FnMut(web_sys::Event)>>,
    }

    impl WebShell {
        fn new(
            document: &web_sys::Document,
            proxy: EventLoopProxy<WebEvent>,
        ) -> Result<Self, String> {
            let body = document
                .body()
                .ok_or_else(|| String::from("document body should exist"))?;

            let root = html_element(
                document,
                "div",
                "position:fixed; inset:0; z-index:10; padding:24px; box-sizing:border-box; \
                 font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',sans-serif; \
                 color:#0f172a; pointer-events:none;",
                None,
            )?;

            let home_panel = html_element(
                document,
                "div",
                "pointer-events:auto; width:min(560px, calc(100vw - 48px)); margin:0 auto; \
                 background:rgba(249,251,253,0.98); border:1px solid #dde4ec; border-radius:8px; \
                 box-shadow:0 18px 48px rgba(15,23,42,0.08); padding:24px; display:flex; \
                 flex-direction:column; gap:16px;",
                None,
            )?;

            let title = html_element(
                document,
                "div",
                "font-size:34px; font-weight:700; letter-spacing:0; color:#0f172a;",
                Some("PianoPro"),
            )?;
            let subtitle = html_element(
                document,
                "div",
                "font-size:15px; line-height:1.5; color:#64748b;",
                Some("Open a MIDI file in the browser or launch the embedded demo."),
            )?;

            let actions = html_element(
                document,
                "div",
                "display:flex; flex-wrap:wrap; gap:12px;",
                None,
            )?;
            let demo_button = html_element(document, "button", "", Some("Play Demo"))?;
            let import_button = html_element(document, "button", "", Some("Import MIDI"))?;
            let start_button = html_element(document, "button", "", Some("Start Selected"))?;

            let selected_card = html_element(
                document,
                "div",
                "display:flex; flex-direction:column; gap:6px; padding:16px; background:#ffffff; \
                 border:1px solid #dde4ec; border-radius:8px;",
                None,
            )?;
            let selected_label = html_element(
                document,
                "div",
                "font-size:11px; font-weight:700; color:#64748b; text-transform:uppercase;",
                Some("Selected piece"),
            )?;
            let selected_title = html_element(
                document,
                "div",
                "font-size:20px; font-weight:700; color:#0f172a;",
                Some("No MIDI selected"),
            )?;
            let selected_meta = html_element(
                document,
                "div",
                "font-size:13px; color:#64748b;",
                Some("Import a MIDI file to add it to the current browser session."),
            )?;

            let status_text = html_element(
                document,
                "div",
                "min-height:20px; font-size:13px; color:#64748b;",
                None,
            )?;
            let error_text = html_element(
                document,
                "div",
                "min-height:20px; font-size:13px; color:#a23c4c;",
                None,
            )?;

            let library_section = html_element(
                document,
                "div",
                "display:flex; flex-direction:column; gap:10px;",
                None,
            )?;
            let library_label = html_element(
                document,
                "div",
                "font-size:11px; font-weight:700; color:#64748b; text-transform:uppercase;",
                Some("Session library"),
            )?;
            let library_list = html_element(
                document,
                "div",
                "display:flex; flex-direction:column; gap:8px; max-height:280px; overflow:auto;",
                None,
            )?;

            let playback_bar = html_element(
                document,
                "div",
                "display:none; pointer-events:auto; align-items:center; justify-content:space-between; \
                 gap:16px; width:min(920px, calc(100vw - 48px)); margin:0 auto;",
                None,
            )?;
            let playback_pill = html_element(
                document,
                "div",
                "display:flex; align-items:center; justify-content:space-between; gap:16px; \
                 width:100%; padding:12px 16px; background:rgba(249,251,253,0.96); \
                 border:1px solid #dde4ec; border-radius:8px; box-shadow:0 12px 32px rgba(15,23,42,0.08);",
                None,
            )?;
            let playback_title = html_element(
                document,
                "div",
                "font-size:15px; font-weight:600; color:#0f172a;",
                Some("Playback"),
            )?;
            let back_button = html_element(document, "button", "", Some("Back to Home"))?;

            set_button_style(&demo_button, true, false)?;
            set_button_style(&import_button, true, false)?;
            set_button_style(&start_button, false, true)?;
            set_button_style(&back_button, true, false)?;

            append(&actions, &demo_button)?;
            append(&actions, &import_button)?;
            append(&actions, &start_button)?;

            append(&selected_card, &selected_label)?;
            append(&selected_card, &selected_title)?;
            append(&selected_card, &selected_meta)?;

            append(&library_section, &library_label)?;
            append(&library_section, &library_list)?;

            append(&home_panel, &title)?;
            append(&home_panel, &subtitle)?;
            append(&home_panel, &actions)?;
            append(&home_panel, &selected_card)?;
            append(&home_panel, &status_text)?;
            append(&home_panel, &error_text)?;
            append(&home_panel, &library_section)?;

            append(&playback_pill, &playback_title)?;
            append(&playback_pill, &back_button)?;
            append(&playback_bar, &playback_pill)?;

            append(&root, &home_panel)?;
            append(&root, &playback_bar)?;
            append(&body, &root)?;

            let mut shell = Self {
                document: document.clone(),
                proxy,
                _root: root,
                home_panel,
                playback_bar,
                library_list,
                selected_title,
                selected_meta,
                status_text,
                error_text,
                playback_title,
                start_button,
                demo_button,
                import_button,
                back_button,
                static_listeners: Vec::new(),
                song_listeners: Vec::new(),
            };
            shell.bind_static_actions()?;
            Ok(shell)
        }

        fn bind_static_actions(&mut self) -> Result<(), String> {
            let demo_proxy = self.proxy.clone();
            let demo_click = Closure::wrap(Box::new(move |_event: web_sys::Event| {
                demo_proxy.send_event(WebEvent::PlayDemo).ok();
            }) as Box<dyn FnMut(_)>);
            self.demo_button
                .add_event_listener_with_callback("click", demo_click.as_ref().unchecked_ref())
                .map_err(|_| String::from("failed to attach demo button listener"))?;
            self.static_listeners.push(demo_click);

            let import_proxy = self.proxy.clone();
            let import_click = Closure::wrap(Box::new(move |_event: web_sys::Event| {
                import_proxy.send_event(WebEvent::ImportStarted).ok();

                let import_proxy = import_proxy.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    let file = AsyncFileDialog::new()
                        .add_filter("midi", &["mid", "midi"])
                        .pick_file()
                        .await;

                    let Some(file) = file else {
                        import_proxy.send_event(WebEvent::ImportCanceled).ok();
                        return;
                    };

                    let file_name = file.file_name();
                    let bytes = file.read().await;
                    let result = midi_file::MidiFile::from_bytes(file_name.clone(), &bytes)
                        .map(|_| WebSong::imported(file_name, bytes))
                        .map_err(|err| format!("failed to import MIDI: {err}"));

                    import_proxy.send_event(WebEvent::SongImported(result)).ok();
                });
            }) as Box<dyn FnMut(_)>);
            self.import_button
                .add_event_listener_with_callback("click", import_click.as_ref().unchecked_ref())
                .map_err(|_| String::from("failed to attach import button listener"))?;
            self.static_listeners.push(import_click);

            let start_proxy = self.proxy.clone();
            let start_click = Closure::wrap(Box::new(move |_event: web_sys::Event| {
                start_proxy.send_event(WebEvent::StartSelected).ok();
            }) as Box<dyn FnMut(_)>);
            self.start_button
                .add_event_listener_with_callback("click", start_click.as_ref().unchecked_ref())
                .map_err(|_| String::from("failed to attach start button listener"))?;
            self.static_listeners.push(start_click);

            let back_proxy = self.proxy.clone();
            let back_click = Closure::wrap(Box::new(move |_event: web_sys::Event| {
                back_proxy.send_event(WebEvent::ReturnHome).ok();
            }) as Box<dyn FnMut(_)>);
            self.back_button
                .add_event_listener_with_callback("click", back_click.as_ref().unchecked_ref())
                .map_err(|_| String::from("failed to attach back button listener"))?;
            self.static_listeners.push(back_click);

            Ok(())
        }

        fn render_home(
            &mut self,
            songs: &[WebSong],
            selected_song: Option<usize>,
            loading_message: Option<&str>,
            error_message: Option<&str>,
        ) -> Result<(), String> {
            self.home_panel
                .set_attribute(
                    "style",
                    "pointer-events:auto; width:min(560px, calc(100vw - 48px)); margin:0 auto; \
                     background:rgba(249,251,253,0.98); border:1px solid #dde4ec; border-radius:8px; \
                     box-shadow:0 18px 48px rgba(15,23,42,0.08); padding:24px; display:flex; \
                     flex-direction:column; gap:16px;",
                )
                .map_err(|_| String::from("failed to show home panel"))?;
            self.playback_bar
                .set_attribute("style", "display:none;")
                .map_err(|_| String::from("failed to hide playback bar"))?;

            let selected = selected_song.and_then(|index| songs.get(index));
            self.selected_title.set_text_content(Some(
                selected
                    .map(|song| song.display_name.as_str())
                    .unwrap_or("No MIDI selected"),
            ));
            self.selected_meta.set_text_content(Some(match selected {
                Some(song) => song.origin_label(),
                None => "Import a MIDI file to add it to the current browser session.",
            }));

            self.status_text.set_text_content(loading_message);
            self.error_text.set_text_content(error_message);

            let buttons_enabled = loading_message.is_none();
            set_button_style(&self.demo_button, buttons_enabled, false)?;
            set_button_style(&self.import_button, buttons_enabled, false)?;
            set_button_style(
                &self.start_button,
                buttons_enabled && selected.is_some(),
                true,
            )?;

            self.library_list.set_inner_html("");
            self.song_listeners.clear();

            if songs.is_empty() {
                let empty = html_element(
                    &self.document,
                    "div",
                    "padding:14px 16px; background:#ffffff; border:1px dashed #dde4ec; \
                     border-radius:8px; font-size:14px; color:#64748b;",
                    Some("No imported MIDI yet."),
                )?;
                append(&self.library_list, &empty)?;
                return Ok(());
            }

            for (index, song) in songs.iter().enumerate() {
                let is_selected = Some(index) == selected_song;
                let button = html_element(&self.document, "button", "", None)?;
                let button_style = if is_selected {
                    "display:flex; flex-direction:column; align-items:flex-start; gap:4px; width:100%; \
                     padding:14px 16px; background:#e5eefb; border:1px solid #184087; border-radius:8px; \
                     color:#0f172a; cursor:pointer; text-align:left;"
                } else {
                    "display:flex; flex-direction:column; align-items:flex-start; gap:4px; width:100%; \
                     padding:14px 16px; background:#ffffff; border:1px solid #dde4ec; border-radius:8px; \
                     color:#0f172a; cursor:pointer; text-align:left;"
                };
                button
                    .set_attribute("style", button_style)
                    .map_err(|_| String::from("failed to style library button"))?;

                let title = html_element(
                    &self.document,
                    "div",
                    "font-size:15px; font-weight:600; color:#0f172a;",
                    Some(&song.display_name),
                )?;
                let meta = html_element(
                    &self.document,
                    "div",
                    "font-size:12px; color:#64748b;",
                    Some(song.origin_label()),
                )?;

                append(&button, &title)?;
                append(&button, &meta)?;

                let proxy = self.proxy.clone();
                let song_click = Closure::wrap(Box::new(move |_event: web_sys::Event| {
                    proxy.send_event(WebEvent::SelectSong(index)).ok();
                }) as Box<dyn FnMut(_)>);
                button
                    .add_event_listener_with_callback("click", song_click.as_ref().unchecked_ref())
                    .map_err(|_| String::from("failed to attach library item listener"))?;
                self.song_listeners.push(song_click);

                append(&self.library_list, &button)?;
            }

            Ok(())
        }

        fn show_playback(&mut self, title: &str) -> Result<(), String> {
            self.home_panel
                .set_attribute("style", "display:none;")
                .map_err(|_| String::from("failed to hide home panel"))?;
            self.playback_bar
                .set_attribute(
                    "style",
                    "display:flex; pointer-events:auto; align-items:center; justify-content:space-between; \
                     gap:16px; width:min(920px, calc(100vw - 48px)); margin:0 auto;",
                )
                .map_err(|_| String::from("failed to show playback bar"))?;
            self.playback_title.set_text_content(Some(title));
            Ok(())
        }
    }

    struct WebBootstrap {
        canvas: web_sys::HtmlCanvasElement,
        proxy: EventLoopProxy<WebEvent>,
        app: Rc<RefCell<Option<WebApp>>>,
        shell: WebShell,
        initializing: bool,
        library: Vec<WebSong>,
        selected_song: Option<usize>,
        loading_message: Option<String>,
        error_message: Option<String>,
    }

    impl WebBootstrap {
        fn sync_home(&mut self) {
            if let Err(err) = self.shell.render_home(
                &self.library,
                self.selected_song,
                self.loading_message.as_deref(),
                self.error_message.as_deref(),
            ) {
                log_browser_error(&err);
            }
        }

        fn persist_library(&self) {
            if let Err(err) = save_library_to_storage(&self.library, self.selected_song) {
                log_browser_error(&err);
            }
        }

        fn ensure_app_ready(&mut self) -> bool {
            if self.app.borrow().is_some() {
                true
            } else {
                self.error_message = Some(String::from("WebGPU is still initializing."));
                self.sync_home();
                false
            }
        }
    }

    impl ApplicationHandler<WebEvent> for WebBootstrap {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            if self.initializing || self.app.borrow().is_some() {
                return;
            }
            self.initializing = true;
            self.loading_message = Some(String::from("Starting WebGPU..."));
            self.error_message = None;
            self.sync_home();

            let attributes = Window::default_attributes()
                .with_title("PianoPro")
                .with_canvas(Some(self.canvas.clone()));
            let window = match event_loop.create_window(attributes) {
                Ok(window) => Arc::new(window),
                Err(err) => {
                    self.initializing = false;
                    self.loading_message = None;
                    self.error_message = Some(format!("failed to create web window: {err}"));
                    self.sync_home();
                    return;
                }
            };

            let app_cell = self.app.clone();
            let proxy = self.proxy.clone();
            let canvas = self.canvas.clone();
            wasm_bindgen_futures::spawn_local(async move {
                match WebApp::new(window, canvas).await {
                    Ok(app) => {
                        app.window.request_redraw();
                        app_cell.borrow_mut().replace(app);
                        proxy.send_event(WebEvent::AppInitialized).ok();
                    }
                    Err(err) => {
                        proxy.send_event(WebEvent::AppInitFailed(err)).ok();
                    }
                }
            });
        }

        fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: WebEvent) {
            match event {
                WebEvent::AppInitialized => {
                    self.initializing = false;
                    self.loading_message = None;
                    self.error_message = None;
                    self.sync_home();

                    if let Some(app) = self.app.borrow().as_ref() {
                        app.window.request_redraw();
                    }
                }
                WebEvent::AppInitFailed(err) => {
                    self.initializing = false;
                    self.loading_message = None;
                    self.error_message = Some(err);
                    self.sync_home();
                }
                WebEvent::PlayDemo => {
                    if !self.ensure_app_ready() {
                        return;
                    }

                    let audio_start = {
                        let mut app = self.app.borrow_mut();
                        let Some(app) = app.as_mut() else {
                            return;
                        };
                        app.ensure_audio_started()
                    };

                    if let Err(err) = audio_start {
                        self.error_message = Some(err);
                        self.sync_home();
                        return;
                    }

                    let result = {
                        let mut app = self.app.borrow_mut();
                        let Some(app) = app.as_mut() else {
                            return;
                        };

                        let result = app.load_song(WebSong::demo());
                        if result.is_ok() {
                            app.window.request_redraw();
                        }
                        result
                    };

                    match result {
                        Ok(title) => {
                            self.error_message = None;
                            if let Err(err) = self.shell.show_playback(&title) {
                                log_browser_error(&err);
                            }
                        }
                        Err(err) => {
                            self.error_message = Some(err);
                            self.sync_home();
                        }
                    }
                }
                WebEvent::ImportStarted => {
                    if !self.ensure_app_ready() {
                        return;
                    }

                    self.loading_message = Some(String::from("Opening MIDI picker..."));
                    self.error_message = None;
                    self.sync_home();
                }
                WebEvent::ImportCanceled => {
                    self.loading_message = None;
                    self.sync_home();
                }
                WebEvent::SongImported(result) => {
                    self.loading_message = None;
                    match result {
                        Ok(song) => {
                            self.library.push(song);
                            self.selected_song = Some(self.library.len() - 1);
                            self.persist_library();
                            self.error_message = None;
                        }
                        Err(err) => {
                            self.error_message = Some(err);
                        }
                    }
                    self.sync_home();
                }
                WebEvent::SelectSong(index) => {
                    self.selected_song = Some(index);
                    self.persist_library();
                    self.error_message = None;
                    self.sync_home();
                }
                WebEvent::StartSelected => {
                    if !self.ensure_app_ready() {
                        return;
                    }

                    let Some(index) = self.selected_song else {
                        return;
                    };
                    let Some(song) = self.library.get(index).cloned() else {
                        return;
                    };

                    let audio_start = {
                        let mut app = self.app.borrow_mut();
                        let Some(app) = app.as_mut() else {
                            return;
                        };
                        app.ensure_audio_started()
                    };

                    if let Err(err) = audio_start {
                        self.error_message = Some(err);
                        self.sync_home();
                        return;
                    }

                    let result = {
                        let mut app = self.app.borrow_mut();
                        let Some(app) = app.as_mut() else {
                            return;
                        };

                        let result = app.load_song(song);
                        if result.is_ok() {
                            app.window.request_redraw();
                        }
                        result
                    };

                    match result {
                        Ok(title) => {
                            self.error_message = None;
                            if let Err(err) = self.shell.show_playback(&title) {
                                log_browser_error(&err);
                            }
                        }
                        Err(err) => {
                            self.error_message = Some(err);
                            self.sync_home();
                        }
                    }
                }
                WebEvent::ReturnHome => {
                    if let Some(app) = self.app.borrow_mut().as_mut() {
                        app.unload_song();
                        app.window.request_redraw();
                    }

                    self.error_message = None;
                    self.loading_message = None;
                    self.sync_home();
                }
            }
        }

        fn window_event(
            &mut self,
            event_loop: &ActiveEventLoop,
            _window_id: winit::window::WindowId,
            event: WindowEvent,
        ) {
            let mut app = self.app.borrow_mut();
            let Some(app) = app.as_mut() else {
                if let WindowEvent::CloseRequested = event {
                    event_loop.exit();
                }
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
            if let Some(app) = app.as_ref()
                && app.has_playback()
            {
                app.window.request_redraw();
            }
        }
    }

    fn html_element(
        document: &web_sys::Document,
        tag: &str,
        style: &str,
        text: Option<&str>,
    ) -> Result<web_sys::HtmlElement, String> {
        let element = document
            .create_element(tag)
            .map_err(|_| format!("failed to create <{tag}> element"))?
            .dyn_into::<web_sys::HtmlElement>()
            .map_err(|_| format!("failed to cast <{tag}> to HtmlElement"))?;

        if !style.is_empty() {
            element
                .set_attribute("style", style)
                .map_err(|_| format!("failed to style <{tag}> element"))?;
        }
        if let Some(text) = text {
            element.set_text_content(Some(text));
        }

        Ok(element)
    }

    fn append(parent: &web_sys::HtmlElement, child: &web_sys::HtmlElement) -> Result<(), String> {
        parent
            .append_child(child)
            .map_err(|_| String::from("failed to append DOM element"))?;
        Ok(())
    }

    fn set_button_style(
        button: &web_sys::HtmlElement,
        enabled: bool,
        primary: bool,
    ) -> Result<(), String> {
        let style = match (enabled, primary) {
            (true, true) => {
                "display:inline-flex; align-items:center; justify-content:center; min-height:44px; \
                 padding:0 16px; background:#184087; color:#ffffff; border:1px solid #184087; \
                 border-radius:8px; font-size:14px; font-weight:600; cursor:pointer;"
            }
            (true, false) => {
                "display:inline-flex; align-items:center; justify-content:center; min-height:44px; \
                 padding:0 16px; background:#ffffff; color:#0f172a; border:1px solid #dde4ec; \
                 border-radius:8px; font-size:14px; font-weight:600; cursor:pointer;"
            }
            (false, true) => {
                "display:inline-flex; align-items:center; justify-content:center; min-height:44px; \
                 padding:0 16px; background:#9fb5da; color:#ffffff; border:1px solid #9fb5da; \
                 border-radius:8px; font-size:14px; font-weight:600; cursor:default; opacity:0.75; \
                 pointer-events:none;"
            }
            (false, false) => {
                "display:inline-flex; align-items:center; justify-content:center; min-height:44px; \
                 padding:0 16px; background:#ffffff; color:#64748b; border:1px solid #dde4ec; \
                 border-radius:8px; font-size:14px; font-weight:600; cursor:default; opacity:0.75; \
                 pointer-events:none;"
            }
        };

        button
            .set_attribute("style", style)
            .map_err(|_| String::from("failed to style button"))
    }

    fn initial_surface_size(
        window: &Window,
        canvas: &web_sys::HtmlCanvasElement,
    ) -> PhysicalSize<u32> {
        let size = window.inner_size();
        if size.width > 0 && size.height > 0 {
            return size;
        }

        let scale_factor = web_sys::window()
            .map(|window| window.device_pixel_ratio())
            .unwrap_or(1.0);
        let width = (f64::from(canvas.client_width()).max(1.0) * scale_factor).round() as u32;
        let height = (f64::from(canvas.client_height()).max(1.0) * scale_factor).round() as u32;

        PhysicalSize::new(width.max(1), height.max(1))
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

    const WEB_LIBRARY_STORAGE_KEY: &str = "pianopro.web.library.v1";

    fn load_library_from_storage() -> Result<(Vec<WebSong>, Option<usize>), String> {
        let storage = web_sys::window()
            .ok_or_else(|| String::from("window should exist"))?
            .local_storage()
            .map_err(|_| String::from("failed to access localStorage"))?
            .ok_or_else(|| String::from("localStorage is unavailable"))?;

        let Some(raw) = storage
            .get_item(WEB_LIBRARY_STORAGE_KEY)
            .map_err(|_| String::from("failed to read web library state"))?
        else {
            return Ok((Vec::new(), None));
        };

        let persisted: PersistedWebLibrary = serde_json::from_str(&raw)
            .map_err(|err| format!("failed to deserialize web library state: {err}"))?;

        if persisted.version != 1 {
            return Err(format!(
                "unsupported web library state version: {}",
                persisted.version
            ));
        }

        let mut songs = Vec::with_capacity(persisted.songs.len());
        for song in persisted.songs {
            songs.push(song.into_song()?);
        }

        let selected_song = persisted.selected_song.filter(|index| *index < songs.len());

        Ok((songs, selected_song))
    }

    fn save_library_to_storage(
        songs: &[WebSong],
        selected_song: Option<usize>,
    ) -> Result<(), String> {
        let storage = web_sys::window()
            .ok_or_else(|| String::from("window should exist"))?
            .local_storage()
            .map_err(|_| String::from("failed to access localStorage"))?
            .ok_or_else(|| String::from("localStorage is unavailable"))?;

        let persisted = PersistedWebLibrary {
            version: 1,
            songs: songs
                .iter()
                .filter_map(PersistedWebSong::from_song)
                .collect(),
            selected_song,
        };

        let payload = serde_json::to_string(&persisted)
            .map_err(|err| format!("failed to serialize web library state: {err}"))?;

        storage
            .set_item(WEB_LIBRARY_STORAGE_KEY, &payload)
            .map_err(|_| String::from("failed to write web library state"))?;

        Ok(())
    }

    fn now_seconds() -> f64 {
        web_sys::window()
            .and_then(|window| window.performance())
            .map(|performance| performance.now() / 1000.0)
            .unwrap_or(0.0)
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

        let event_loop: EventLoop<WebEvent> = EventLoop::with_user_event().build().unwrap();
        let proxy = event_loop.create_proxy();
        let shell = WebShell::new(&document, proxy.clone()).expect("failed to create web shell");
        let (library, selected_song) = match load_library_from_storage() {
            Ok(state) => state,
            Err(err) => {
                log_browser_error(&err);
                (Vec::new(), None)
            }
        };

        let mut bootstrap = WebBootstrap {
            canvas,
            proxy,
            app: Rc::new(RefCell::new(None)),
            shell,
            initializing: false,
            library,
            selected_song,
            loading_message: Some(String::from("Starting WebGPU...")),
            error_message: None,
        };
        bootstrap.sync_home();

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
