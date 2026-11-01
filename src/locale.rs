//! Textos del juego en Español e Inglés, seleccionados según los ajustes o
//! el idioma del sistema.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Language {
    Spanish,
    English,
}

impl Default for Language {
    fn default() -> Self {
        Self::detect_system()
    }
}

pub const LANGUAGES: [Language; 2] = [Language::Spanish, Language::English];

impl Language {
    /// Detecta el idioma del sistema con fallback a inglés si no es español.
    pub fn detect_system() -> Self {
        if sys_locale::get_locale().is_some_and(|l| l.to_lowercase().starts_with("es")) {
            return Language::Spanish;
        }
        Language::English
    }

    pub fn name(self) -> &'static str {
        match self {
            Language::Spanish => "Español",
            Language::English => "English",
        }
    }

    // --- Menú Bienvenida / Opciones ---
    pub fn start(self) -> &'static str {
        match self {
            Language::Spanish => "Comenzar",
            Language::English => "Start",
        }
    }

    pub fn options(self) -> &'static str {
        match self {
            Language::Spanish => "Opciones",
            Language::English => "Options",
        }
    }

    pub fn fullscreen(self) -> &'static str {
        match self {
            Language::Spanish => "Pantalla completa",
            Language::English => "Fullscreen",
        }
    }

    pub fn fullscreen_kind(self) -> &'static str {
        match self {
            Language::Spanish => "Tipo de pantalla completa",
            Language::English => "Fullscreen type",
        }
    }

    pub fn resolution(self) -> &'static str {
        match self {
            Language::Spanish => "Resolución",
            Language::English => "Resolution",
        }
    }

    pub fn fps(self) -> &'static str {
        "FPS"
    }

    pub fn language(self) -> &'static str {
        match self {
            Language::Spanish => "Idioma",
            Language::English => "Language",
        }
    }

    pub fn controls(self) -> &'static str {
        match self {
            Language::Spanish => "Controles",
            Language::English => "Controls",
        }
    }

    pub fn back(self) -> &'static str {
        match self {
            Language::Spanish => "Volver",
            Language::English => "Back",
        }
    }

    pub fn dynamic(self) -> &'static str {
        match self {
            Language::Spanish => "Dinámica",
            Language::English => "Dynamic",
        }
    }

    pub fn whole_page(self) -> &'static str {
        match self {
            Language::Spanish => "Toda la página",
            Language::English => "Full page",
        }
    }

    pub fn fps_screen(self) -> &'static str {
        match self {
            Language::Spanish => "De la pantalla",
            Language::English => "Screen",
        }
    }

    pub fn fps_unlimited(self) -> &'static str {
        match self {
            Language::Spanish => "Sin límite",
            Language::English => "Unlimited",
        }
    }

    pub fn controls_hint(self) -> &'static str {
        match self {
            Language::Spanish => "Pulsa una tecla o botón del mando (Esc para cancelar)",
            Language::English => "Press a key or gamepad button (Esc to cancel)",
        }
    }

    pub fn steering_rate(self) -> &'static str {
        match self {
            Language::Spanish => "Sensibilidad giro",
            Language::English => "Steering Rate",
        }
    }

    // --- Acciones de control ---
    pub fn action_throttle(self) -> &'static str {
        match self {
            Language::Spanish => "Acelerar",
            Language::English => "Throttle",
        }
    }

    pub fn action_brake(self) -> &'static str {
        match self {
            Language::Spanish => "Frenar / Marcha atrás",
            Language::English => "Brake / Reverse",
        }
    }

    pub fn action_left(self) -> &'static str {
        match self {
            Language::Spanish => "Girar a la izquierda",
            Language::English => "Steer Left",
        }
    }

    pub fn action_right(self) -> &'static str {
        match self {
            Language::Spanish => "Girar a la derecha",
            Language::English => "Steer Right",
        }
    }

    pub fn action_wheelie(self) -> &'static str {
        match self {
            Language::Spanish => "Caballito",
            Language::English => "Wheelie",
        }
    }

    pub fn action_look_back(self) -> &'static str {
        match self {
            Language::Spanish => "Mirar atrás",
            Language::English => "Look Back",
        }
    }

    pub fn action_camera(self) -> &'static str {
        match self {
            Language::Spanish => "Cambiar cámara",
            Language::English => "Change Camera",
        }
    }

    pub fn action_pause(self) -> &'static str {
        match self {
            Language::Spanish => "Pausa",
            Language::English => "Pause",
        }
    }

    // --- 2 Jugadores Locales / Lobby ---
    pub fn select_players(self) -> &'static str {
        match self {
            Language::Spanish => "ELIGE JUGADOR",
            Language::English => "CHOOSE PLAYER",
        }
    }

    pub fn select_players_subtitle(self) -> &'static str {
        match self {
            Language::Spanish => "Player 1 a la izquierda   ·   Player 2 a la derecha",
            Language::English => "Player 1 on the left   ·   Player 2 on the right",
        }
    }

    pub fn player_1(self) -> &'static str {
        match self {
            Language::Spanish => "PLAYER 1 (IZQUIERDA)",
            Language::English => "PLAYER 1 (LEFT)",
        }
    }

    pub fn player_2(self) -> &'static str {
        match self {
            Language::Spanish => "PLAYER 2 (DERECHA)",
            Language::English => "PLAYER 2 (RIGHT)",
        }
    }

    pub fn player_waiting_left(self) -> &'static str {
        match self {
            Language::Spanish => "Pulsa A o Izquierda para elegir este lado",
            Language::English => "Press A or Left to choose this side",
        }
    }

    pub fn player_waiting_right(self) -> &'static str {
        match self {
            Language::Spanish => "Pulsa D o Derecha para elegir este lado",
            Language::English => "Press D or Right to choose this side",
        }
    }

    pub fn player_stick_tip(self) -> &'static str {
        match self {
            Language::Spanish => "O mueve el stick del mando",
            Language::English => "Or move gamepad stick",
        }
    }

    pub fn player_ready(self) -> &'static str {
        match self {
            Language::Spanish => "¡LISTO!",
            Language::English => "READY!",
        }
    }

    pub fn waiting_player(self) -> &'static str {
        match self {
            Language::Spanish => "ESPERANDO JUGADOR...",
            Language::English => "WAITING FOR PLAYER...",
        }
    }

    // --- Pistas ---
    pub fn select_track(self) -> &'static str {
        match self {
            Language::Spanish => "ELIGE PISTA",
            Language::English => "SELECT TRACK",
        }
    }

    pub fn track_length_label(self, km: &str, bikes: usize) -> String {
        match self {
            Language::Spanish => format!("{km} km por vuelta   ·   {bikes} motos"),
            Language::English => format!("{km} km per lap   ·   {bikes} bikes"),
        }
    }

    // --- Motos ---
    pub fn select_bike(self) -> &'static str {
        match self {
            Language::Spanish => "ELIGE MOTO",
            Language::English => "SELECT BIKE",
        }
    }

    pub fn stat_speed(self) -> &'static str {
        match self {
            Language::Spanish => "Velocidad",
            Language::English => "Top Speed",
        }
    }

    pub fn stat_accel(self) -> &'static str {
        match self {
            Language::Spanish => "Aceleración",
            Language::English => "Acceleration",
        }
    }

    pub fn stat_grip(self) -> &'static str {
        match self {
            Language::Spanish => "Agarre",
            Language::English => "Grip",
        }
    }

    // --- HUD y Carrera ---
    pub fn lap(self) -> &'static str {
        match self {
            Language::Spanish => "VUELTA",
            Language::English => "LAP",
        }
    }

    pub fn pos(self) -> &'static str {
        match self {
            Language::Spanish => "POS",
            Language::English => "POS",
        }
    }

    pub fn time(self) -> &'static str {
        match self {
            Language::Spanish => "TIEMPO",
            Language::English => "TIME",
        }
    }

    pub fn best(self) -> &'static str {
        match self {
            Language::Spanish => "MEJOR",
            Language::English => "BEST",
        }
    }

    pub fn finish(self) -> &'static str {
        match self {
            Language::Spanish => "META",
            Language::English => "FINISH",
        }
    }

    pub fn best_lap(self) -> &'static str {
        match self {
            Language::Spanish => "VUELTA RÁPIDA",
            Language::English => "FASTEST LAP",
        }
    }

    pub fn countdown_go(self) -> &'static str {
        match self {
            Language::Spanish => "¡YA!",
            Language::English => "GO!",
        }
    }

    // --- Menú Pausa ---
    pub fn pause_title(self) -> &'static str {
        match self {
            Language::Spanish => "PAUSA",
            Language::English => "PAUSE",
        }
    }

    pub fn resume(self) -> &'static str {
        match self {
            Language::Spanish => "Continuar",
            Language::English => "Resume",
        }
    }

    pub fn restart(self) -> &'static str {
        match self {
            Language::Spanish => "Reiniciar",
            Language::English => "Restart",
        }
    }

    pub fn choose_bike_item(self) -> &'static str {
        match self {
            Language::Spanish => "Elegir moto",
            Language::English => "Choose bike",
        }
    }

    pub fn choose_track_item(self) -> &'static str {
        match self {
            Language::Spanish => "Elegir pista",
            Language::English => "Choose track",
        }
    }

    pub fn main_menu_item(self) -> &'static str {
        match self {
            Language::Spanish => "Menú principal",
            Language::English => "Main menu",
        }
    }

    // --- Gamepad / Mandos ---
    pub fn gamepad_connected(self, name: &str) -> String {
        match self {
            Language::Spanish => format!("Mando conectado: {name}"),
            Language::English => format!("Gamepad connected: {name}"),
        }
    }

    pub fn no_gamepad(self) -> &'static str {
        match self {
            Language::Spanish => "Sin mando (teclas en Opciones)",
            Language::English => "No gamepad (keys in Options)",
        }
    }

    pub fn exit_game(self) -> &'static str {
        match self {
            Language::Spanish => "Salir del juego",
            Language::English => "Exit game",
        }
    }

    pub fn tab_game(self) -> &'static str {
        match self {
            Language::Spanish => "Juego",
            Language::English => "Game",
        }
    }

    pub fn tab_controls(self) -> &'static str {
        match self {
            Language::Spanish => "Control",
            Language::English => "Control",
        }
    }

    pub fn tab_sound(self) -> &'static str {
        match self {
            Language::Spanish => "Sonido",
            Language::English => "Sound",
        }
    }

    pub fn tab_graphics(self) -> &'static str {
        match self {
            Language::Spanish => "Gráficos",
            Language::English => "Graphics",
        }
    }

    pub fn sound_fx_volume(self) -> &'static str {
        match self {
            Language::Spanish => "Volumen FX",
            Language::English => "FX Volume",
        }
    }

    pub fn sound_music_volume(self) -> &'static str {
        match self {
            Language::Spanish => "Volumen música",
            Language::English => "Music Volume",
        }
    }

    pub fn sound_music(self) -> &'static str {
        match self {
            Language::Spanish => "Música",
            Language::English => "Music",
        }
    }

    pub fn enabled(self) -> &'static str {
        match self {
            Language::Spanish => "Activada",
            Language::English => "Enabled",
        }
    }

    pub fn disabled(self) -> &'static str {
        match self {
            Language::Spanish => "Desactivada",
            Language::English => "Disabled",
        }
    }

    pub fn show_legend(self) -> &'static str {
        match self {
            Language::Spanish => "Leyenda de controles",
            Language::English => "Controls legend",
        }
    }

    pub fn rescan_gamepads(self) -> &'static str {
        match self {
            Language::Spanish => "Volver a buscar mandos",
            Language::English => "Rescan gamepads",
        }
    }

    pub fn one_player(self) -> &'static str {
        match self {
            Language::Spanish => "1 jugador",
            Language::English => "1 player",
        }
    }

    pub fn two_players(self) -> &'static str {
        match self {
            Language::Spanish => "2 jugadores",
            Language::English => "2 players",
        }
    }

    pub fn return_to_main_menu(self) -> &'static str {
        match self {
            Language::Spanish => "Volver al menú principal",
            Language::English => "Return to main menu",
        }
    }

    pub fn reset(self) -> &'static str {
        match self {
            Language::Spanish => "Restablecer",
            Language::English => "Reset",
        }
    }

    // --- Controls legend: the same words on every screen ---

    pub fn hint_accept(self) -> &'static str {
        match self {
            Language::Spanish => "Aceptar",
            Language::English => "Accept",
        }
    }

    pub fn hint_resume(self) -> &'static str {
        match self {
            Language::Spanish => "Continuar",
            Language::English => "Resume",
        }
    }

    pub fn hint_cancel(self) -> &'static str {
        match self {
            Language::Spanish => "Cancelar",
            Language::English => "Cancel",
        }
    }

    pub fn hint_exit(self) -> &'static str {
        match self {
            Language::Spanish => "Salir",
            Language::English => "Exit",
        }
    }

    pub fn hint_choose_bike(self) -> &'static str {
        match self {
            Language::Spanish => "Elegir moto",
            Language::English => "Choose bike",
        }
    }

    pub fn hint_race(self) -> &'static str {
        match self {
            Language::Spanish => "Correr",
            Language::English => "Race",
        }
    }

    pub fn hint_continue(self) -> &'static str {
        match self {
            Language::Spanish => "Continuar",
            Language::English => "Continue",
        }
    }

    pub fn hint_brake(self) -> &'static str {
        match self {
            Language::Spanish => "Frenar",
            Language::English => "Brake",
        }
    }

    pub fn hint_steer(self) -> &'static str {
        match self {
            Language::Spanish => "Girar",
            Language::English => "Steer",
        }
    }

    pub fn hint_sound(self) -> &'static str {
        match self {
            Language::Spanish => "Sonido",
            Language::English => "Sound",
        }
    }
}
