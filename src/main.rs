mod color;
mod config;
mod key;
mod menu;
mod text;

use std::collections::HashMap;
use std::f64::consts::{FRAC_PI_2, PI, TAU};
use std::io;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::LazyLock;

use anyhow::bail;
use clap::Parser;
use pangocairo::cairo;
use smithay_client_toolkit::compositor::{CompositorHandler, CompositorState};
use smithay_client_toolkit::reexports::protocols::wp::keyboard_shortcuts_inhibit::zv1::client::zwp_keyboard_shortcuts_inhibit_manager_v1::ZwpKeyboardShortcutsInhibitManagerV1;
use smithay_client_toolkit::reexports::protocols::wp::keyboard_shortcuts_inhibit::zv1::client::zwp_keyboard_shortcuts_inhibitor_v1::ZwpKeyboardShortcutsInhibitorV1;
use smithay_client_toolkit::registry::{ProvidesRegistryState, RegistryState};
use smithay_client_toolkit::{delegate_compositor, delegate_keyboard, delegate_layer, delegate_output, delegate_registry, delegate_seat, delegate_shm};
use smithay_client_toolkit::output::{OutputHandler, OutputState};
use smithay_client_toolkit::seat::{keyboard::KeyboardHandler, Capability, SeatHandler, SeatState};
use smithay_client_toolkit::shell::wlr_layer::{KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shm::slot::SlotPool;
use smithay_client_toolkit::shm::{Shm, ShmHandler};
use wayland_client::globals::registry_queue_init;
use wayland_client::protocol::wl_keyboard::WlKeyboard;
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::protocol::wl_shm::Format;
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};

use crate::key::ModifierState;

#[derive(Debug, Parser)]
#[command(author, version, about)]
struct Args {
    /// The name of the config file to use.
    ///
    /// By default, $XDG_CONFIG_HOME/wlr-which-key/config.yaml or
    /// ~/.config/wlr-which-key/config.yaml is used.
    ///
    /// For example, to use ~/.config/wlr-which-key/print-srceen.yaml, set this to
    /// "print-srceen". An absolute path can be used too, extension is optional.
    config: Option<String>,

    /// Initial key sequence to navigate to a specific submenu on startup.
    ///
    /// Provide a sequence of keys separated by spaces to navigate directly to a submenu.
    /// For example, "p s" would navigate to the submenu at key 'p', then 's'.
    /// The application will show an error and exit if the key sequence is invalid.
    #[arg(long, short = 'k')]
    initial_keys: Option<String>,
}

static DEBUG_LAYOUT: LazyLock<bool> =
    LazyLock::new(|| std::env::var("WLR_WHICH_KEY_LAYOUT_DEBUG").as_deref() == Ok("1"));

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let config = config::Config::new(args.config.as_deref().unwrap_or("config"))?;
    let mut menu = menu::Menu::new(&config)?;

    if let Some(initial_keys) = &args.initial_keys {
        if let Some(initial_action) = menu.navigate_to_key_sequence(initial_keys)? {
            match initial_action {
                menu::Action::Submenu(_) => unreachable!(),
                menu::Action::Quit => return Ok(()),
                menu::Action::Exec { cmd, keep_open } => {
                    if keep_open {
                        bail!("Initial key sequence cannot trigger an action with keep_open=true");
                    }
                    exec(&cmd);
                    return Ok(());
                }
            }
        }
    }

    let conn = Connection::connect_to_env()?;
    let (globals, mut event_queue) = registry_queue_init(&conn).unwrap();
    let qh = event_queue.handle();

    let registry_state = RegistryState::new(&globals);
    let output = OutputState::new(&globals, &qh);

    let wl_compositor = CompositorState::bind(&globals, &qh).expect("wl_compositor not available");

    let layer_shell = LayerShell::bind(&globals, &qh).expect("layer shell not available");

    let seat = SeatState::new(&globals, &qh);

    let keyboard_shortcuts_inhibit_manager = match config.inhibit_compositor_keyboard_shortcuts {
        true => Some(
            globals
                .bind(&qh, 1..=1, ())
                .expect("zwp_keyboard_shortcuts_inhibit_manager not available"),
        ),
        false => None,
    };

    let shm = Shm::bind(&globals, &qh).expect("wl_shm is not available");

    let width = menu.width(&config) as u32;
    let height = menu.height(&config) as u32;

    let surface = wl_compositor.create_surface(&qh);

    let layer_surface =
        layer_shell.create_layer_surface(&qh, surface, Layer::Overlay, Some("wlr_which_key"), None);
    layer_surface.set_anchor(config.anchor.into());
    layer_surface.set_size(width, height);
    layer_surface.set_margin(
        config.margin_top,
        config.margin_right,
        config.margin_bottom,
        config.margin_left,
    );
    layer_surface.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);

    layer_surface.commit();

    let mut state = State {
        pool: SlotPool::new((width * height * 4) as usize, &shm).unwrap(),
        keyboard_shortcuts_inhibit_manager,
        keyboard_shortcuts_inhibitors: HashMap::new(),

        shm,
        output,
        registry_state,
        layer_surface,
        seat,
        keyboard: None,

        surface_scale: 1,
        exit: false,
        configured: false,
        width,
        height,
        damaged: true,

        menu,
        config,

        modifiers: ModifierState::default(),
    };

    while !state.exit {
        event_queue.blocking_dispatch(&mut state).unwrap();
    }

    Ok(())
}

struct State {
    pool: SlotPool,
    keyboard_shortcuts_inhibit_manager: Option<ZwpKeyboardShortcutsInhibitManagerV1>,
    keyboard_shortcuts_inhibitors: HashMap<WlSeat, ZwpKeyboardShortcutsInhibitorV1>,

    shm: Shm,
    output: OutputState,
    registry_state: RegistryState,
    layer_surface: LayerSurface,
    seat: SeatState,
    keyboard: Option<WlKeyboard>,

    surface_scale: u32,
    exit: bool,
    configured: bool,
    width: u32,
    height: u32,
    damaged: bool,

    menu: menu::Menu,
    config: config::Config,

    modifiers: ModifierState,
}

impl State {
    fn draw(&mut self, _conn: &Connection, qh: &QueueHandle<State>) {
        if !self.configured {
            return;
        }

        if !self.damaged {
            return;
        }

        let scale = self.surface_scale;

        let width_f = self.width as f64;
        let height_f = self.height as f64;

        let (buffer, canvas) = self
            .pool
            .create_buffer(
                (self.width * scale) as i32,
                (self.height * scale) as i32,
                (self.width * 4 * scale) as i32,
                Format::Argb8888,
            )
            .expect("could not allocate frame shm buffer");

        let cairo_surf = unsafe {
            cairo::ImageSurface::create_for_data_unsafe(
                canvas.as_mut_ptr(),
                cairo::Format::ARgb32,
                (self.width/*  * scale */) as i32,
                (self.height/*  * scale */) as i32,
                (self.width * 4/*  * scale */) as i32,
            )
            .expect("cairo surface")
        };

        let cairo_ctx = cairo::Context::new(&cairo_surf).expect("cairo context");
        cairo_ctx.scale(scale as f64, scale as f64);
        self.layer_surface.wl_surface().set_buffer_scale(scale as i32);

        // background with rounded corners
        cairo_ctx.save().unwrap();
        cairo_ctx.set_operator(cairo::Operator::Source);
        color::Color::TRANSPARENT.apply(&cairo_ctx);
        cairo_ctx.paint().unwrap();
        cairo_ctx.restore().unwrap();

        cairo_ctx.new_sub_path();
        let half_border = self.config.border_width * 0.5;
        let r = self.config.corner_r;
        cairo_ctx.arc(r + half_border, r + half_border, r, PI, 3.0 * FRAC_PI_2);
        cairo_ctx.arc(
            width_f - r - half_border,
            r + half_border,
            r,
            3.0 * FRAC_PI_2,
            TAU,
        );
        cairo_ctx.arc(
            width_f - r - half_border,
            height_f - r - half_border,
            r,
            0.0,
            FRAC_PI_2,
        );
        cairo_ctx.arc(
            r + half_border,
            height_f - r - half_border,
            r,
            FRAC_PI_2,
            PI,
        );
        cairo_ctx.close_path();
        self.config.background.apply(&cairo_ctx);
        cairo_ctx.fill_preserve().unwrap();
        self.config.border.apply(&cairo_ctx);
        cairo_ctx.set_line_width(self.config.border_width);
        cairo_ctx.stroke().unwrap();

        // draw our menu
        self.menu.render(&self.config, &cairo_ctx).unwrap();

        // Damage the entire window
        self.layer_surface.wl_surface().damage_buffer(
            0,
            0,
            (self.width * scale) as i32,
            (self.height * scale) as i32,
        );
        self.damaged = false;

        self.layer_surface
            .wl_surface()
            .frame(qh, self.layer_surface.wl_surface().clone());

        // Attach and commit to present.
        buffer.attach_to(self.layer_surface.wl_surface()).unwrap();
        self.layer_surface.wl_surface().commit();
    }

    fn handle_action(&mut self, _conn: &Connection, action: menu::Action) {
        match action {
            menu::Action::Quit => {
                self.exit = true;
            }
            menu::Action::Exec { cmd, keep_open } => {
                exec(&cmd);
                if !keep_open {
                    self.exit = true;
                }
            }
            menu::Action::Submenu(page) => {
                self.menu.set_page(page);
                self.width = self.menu.width(&self.config) as u32;
                self.height = self.menu.height(&self.config) as u32;
                self.layer_surface.set_size(self.width, self.height);
                self.layer_surface.commit();
                self.damaged = true;
            }
        }
    }
}

impl Dispatch<ZwpKeyboardShortcutsInhibitManagerV1, ()> for State {
    fn event(
        _state: &mut Self,
        _proxy: &ZwpKeyboardShortcutsInhibitManagerV1,
        _event: <ZwpKeyboardShortcutsInhibitManagerV1 as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwpKeyboardShortcutsInhibitorV1, ()> for State {
    fn event(
        _state: &mut Self,
        _proxy: &ZwpKeyboardShortcutsInhibitorV1,
        _event: <ZwpKeyboardShortcutsInhibitorV1 as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl CompositorHandler for State {
    fn scale_factor_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wayland_client::protocol::wl_surface::WlSurface,
        new_factor: i32,
    ) {
        let scale = new_factor as u32;
        if scale != self.surface_scale {
            self.surface_scale = scale;
            self.damaged = true;
        }
    }

    fn transform_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wayland_client::protocol::wl_surface::WlSurface,
        _new_transform: wayland_client::protocol::wl_output::Transform,
    ) {
    }

    fn frame(
        &mut self,
        conn: &Connection,
        qh: &QueueHandle<Self>,
        _surface: &wayland_client::protocol::wl_surface::WlSurface,
        _time: u32,
    ) {
        self.draw(conn, qh);
    }

    fn surface_enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wayland_client::protocol::wl_surface::WlSurface,
        _output: &wayland_client::protocol::wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wayland_client::protocol::wl_surface::WlSurface,
        _output: &wayland_client::protocol::wl_output::WlOutput,
    ) {
    }
}

impl OutputHandler for State {
    fn output_state(&mut self) -> &mut smithay_client_toolkit::output::OutputState {
        &mut self.output
    }

    fn new_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wayland_client::protocol::wl_output::WlOutput,
    ) {
    }

    fn update_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wayland_client::protocol::wl_output::WlOutput,
    ) {
    }

    fn output_destroyed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wayland_client::protocol::wl_output::WlOutput,
    ) {
    }
}

impl LayerShellHandler for State {
    fn closed(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _layer: &LayerSurface) {
        self.exit = true
    }

    fn configure(
        &mut self,
        conn: &Connection,
        qh: &QueueHandle<Self>,
        _layer: &LayerSurface,
        configure: smithay_client_toolkit::shell::wlr_layer::LayerSurfaceConfigure,
        _serial: u32,
    ) {
        let size = configure.new_size;
        self.width = size.0;
        self.height = size.1;
        self.configured = true;
        self.draw(conn, qh);
    }
}

impl ProvidesRegistryState for State {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }

    fn runtime_add_global(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _name: u32,
        _interface: &str,
        _version: u32,
    ) {
    }

    fn runtime_remove_global(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _name: u32,
        _interface: &str,
    ) {
    }
}

impl ShmHandler for State {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl SeatHandler for State {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat
    }

    fn new_seat(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        seat: wayland_client::protocol::wl_seat::WlSeat,
    ) {
        if let Some(inhibit_manager) = &self.keyboard_shortcuts_inhibit_manager {
            self.keyboard_shortcuts_inhibitors.insert(
                seat.clone(),
                inhibit_manager.inhibit_shortcuts(self.layer_surface.wl_surface(), &seat, qh, ()),
            );
        }
    }

    fn remove_seat(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        seat: wayland_client::protocol::wl_seat::WlSeat,
    ) {
        if let Some(inhibitor) = self.keyboard_shortcuts_inhibitors.remove(&seat) {
            inhibitor.destroy();
        }
    }

    fn new_capability(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        seat: wayland_client::protocol::wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Keyboard && self.keyboard.is_none() {
            let keyboard = self
                .seat
                .get_keyboard(qh, &seat, None)
                .expect("Failed to create keyboard");
            self.keyboard = Some(keyboard.clone());
        }
    }

    fn remove_capability(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _seat: wayland_client::protocol::wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Keyboard && self.keyboard.is_some() {
            self.keyboard.take().unwrap().release();
        }
    }
}

impl KeyboardHandler for State {
    fn enter(
        &mut self,
        _conn: &Connection,
        _qh: &wayland_client::QueueHandle<Self>,
        _keyboard: &wayland_client::protocol::wl_keyboard::WlKeyboard,
        _surface: &wayland_client::protocol::wl_surface::WlSurface,
        _serial: u32,
        _raw: &[u32],
        _keysyms: &[smithay_client_toolkit::seat::keyboard::Keysym],
    ) {
    }

    fn leave(
        &mut self,
        _conn: &Connection,
        _qh: &wayland_client::QueueHandle<Self>,
        _keyboard: &wayland_client::protocol::wl_keyboard::WlKeyboard,
        _surface: &wayland_client::protocol::wl_surface::WlSurface,
        _serial: u32,
    ) {
    }

    fn press_key(
        &mut self,
        conn: &Connection,
        _qh: &wayland_client::QueueHandle<Self>,
        _keyboard: &wayland_client::protocol::wl_keyboard::WlKeyboard,
        _serial: u32,
        event: smithay_client_toolkit::seat::keyboard::KeyEvent,
    ) {
        let action = if let Some(action) = self.menu.get_action(self.modifiers, event.keysym) {
            Some(action)
        } else {
            None
        };
        if let Some(action) = action {
            self.handle_action(conn, action);
        }
    }

    fn release_key(
        &mut self,
        _conn: &Connection,
        _qh: &wayland_client::QueueHandle<Self>,
        _keyboard: &wayland_client::protocol::wl_keyboard::WlKeyboard,
        _serial: u32,
        _event: smithay_client_toolkit::seat::keyboard::KeyEvent,
    ) {
    }

    fn update_modifiers(
        &mut self,
        _conn: &Connection,
        _qh: &wayland_client::QueueHandle<Self>,
        _keyboard: &wayland_client::protocol::wl_keyboard::WlKeyboard,
        _serial: u32,
        modifiers: smithay_client_toolkit::seat::keyboard::Modifiers,
        _layout: u32,
    ) {
        self.modifiers = ModifierState::from_sctk_modifiers(&modifiers);
    }
}

delegate_compositor!(State);
delegate_output!(State);
delegate_shm!(State);
delegate_seat!(State);
delegate_keyboard!(State);

delegate_layer!(State);
delegate_registry!(State);

fn exec(cmd: &str) {
    let mut proc = Command::new("sh");
    proc.args(["-c", cmd]);
    proc.stdin(Stdio::null());
    proc.stdout(Stdio::null());
    // Safety: libc::daemon() is async-signal-safe
    unsafe {
        proc.pre_exec(|| match libc::daemon(1, 0) {
            -1 => Err(io::Error::other("Failed to detach new process")),
            _ => Ok(()),
        });
    }
    proc.spawn().unwrap().wait().unwrap();
}
