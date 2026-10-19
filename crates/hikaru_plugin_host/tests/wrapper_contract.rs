// Copyright (C) Hikaru Corporation - 2026
// GNU Lesser General Public License v3
// crates/hikaru_plugin_host/tests/wrapper_contract.rs

//! Contrato público del envoltorio "Fruity Wrapper".
//!
//! Son tests de *API*, no de comportamiento interno: corrigen sobre lo que
//! `hikaru_gui` importa, para que un rename accidental o un cambio de firma
//! rompa la compilación acá y no en el DAW.

use hikaru_plugin_host::hikaru_wrapper::{
    plan_hints, CursorHint, FeedbackOrigin, FrameMetrics, FrameRegion, HostFrame, HostRelation,
    InputActivation, InputMask, InputPolicy, LoopExit, ResizeDir, TickBudget, WrapperConfig,
    WrapperKeepAlive, WrapperStyle, WrapperWindowType,
};
use hikaru_plugin_host::HostParentKind;

/// Los tipos que el DAW necesita nombrar.
#[test]
fn la_api_publica_tiene_los_tipos_que_el_daw_necesita() {
    // Compilar es el test: si alguno de estos nombres desaparece, el DAW deja
    // de construir y el error sale acá, no en producción.
    let _: WrapperConfig = WrapperConfig::default();
    let _: WrapperWindowType = WrapperWindowType::Utility;
    let _: InputPolicy = InputPolicy::default();
    let _: InputMask = InputMask::default();
    let _: TickBudget = TickBudget::default();
    let _: FeedbackOrigin = FeedbackOrigin::Editor;
    let _: LoopExit = LoopExit::Requested;
}

/// `PluginWrapperConfig` sigue siendo un alias del nombre viejo.
///
/// El módulo se movió y se renombró cuando se aisló; si alguien fuera a
/// actualizar ese import debería enterarse de que dejó de ser necesario, no
/// de que el tipo desapareció.
#[test]
fn el_alias_historico_sigue_apuntando_al_tipo_nuevo() {
    let nuevo: WrapperConfig = hikaru_plugin_host::PluginWrapperConfig::default();
    assert_eq!(nuevo, WrapperConfig::default());
}

/// Lo que el DAW necesita para abrir un editor como ventana propia.
#[test]
fn hikaru_wrapper_esta_exposto_en_la_raiz_del_crate() {
    // Compilar es el test: si el tipo deja de estar re-exportado, el DAW deja
    // de construir y el error sale acá en vez de en producción.
    let _: fn(
        &std::path::Path,
        hikaru_plugin_host::PluginFormat,
        &str,
        WrapperConfig,
        hikaru_plugin_host::hikaru_wrapper::WrapperRuntime,
        std::sync::mpsc::Receiver<
            hikaru_plugin_host::hikaru_wrapper::WrapperCommand,
        >,
        std::sync::mpsc::Sender<hikaru_plugin_host::hikaru_wrapper::ParamFeedback>,
        hikaru_plugin_host::tap::TapKey,
    ) -> Result<hikaru_plugin_host::HikaruWrapper, hikaru_plugin_host::hikaru_wrapper::WrapperError>
    = hikaru_plugin_host::HikaruWrapper::open;
}

/// El editor es una ventana **del DAW**, no una ventana **del escritorio**.
///
/// El editor de un plugin en Linux es X11 y la sesión es Wayland, así que no
/// hay subsurface embedding posible: lo que se puede es que la ventana sea de
/// Hikaru, sin decoraciones y fuera de la gestión del WM. Esto es lo que
/// distingue "el plugin quedó en una ventana suelta" de "el plugin está en el
/// DAW".
#[test]
fn el_editor_es_una_ventana_de_hikaru_no_una_del_escritorio() {
    let config = WrapperConfig::default();

    assert_eq!(
        config.style,
        WrapperStyle::Owned,
        "el default tiene que ser la ventana del DAW"
    );
    assert!(
        config.override_redirect(),
        "sin override_redirect el WM le pone titlebar, bordes y botón de cerrar"
    );
    assert!(
        !plan_hints(&config).states.contains(&"_NET_WM_STATE_SKIP_TASKBAR")
            || config.skip_taskbar,
        "además no debe aparecer como una aplicación más en el taskbar"
    );
    assert!(config.skip_taskbar, "el editor no es una aplicación independiente");
}

/// "Modal estricta" quiere decir atada a la ventana del DAW: se mueve con
/// ella, se limita a su tamaño y se oculta cuando el DAW se minimiza.
#[test]
fn la_ventana_sigue_a_la_del_daw() {
    let config = WrapperConfig::default().at(HostFrame::new(120, 80, 900, 620));

    assert_eq!(config.relation, HostRelation::StrictChild);
    assert!(config.strict_child(), "con cuadro hay algo a lo que atarse");
    assert!(plan_hints(&config).strict_child);
}

/// Sin que el DAW diga dónde va el editor, no hay nada a lo que atarse: se
/// centra en la pantalla, que es el modo de depuración.
#[test]
fn sin_cuadro_del_daw_la_ventana_se_centra() {
    let config = WrapperConfig::default();
    assert!(config.host_frame.is_none());
    assert!(!config.strict_child());
}

/// El modo flotante es explícito: devuelve el comportamiento de escritorio.
#[test]
fn el_modo_flotante_es_explicito() {
    let config = WrapperConfig::default()
        .at(HostFrame::new(10, 10, 800, 600))
        .floating();

    assert_eq!(config.style, WrapperStyle::Decorated);
    assert_eq!(config.relation, HostRelation::Detached);
    assert!(!config.override_redirect());
    assert!(!config.strict_child());
}

/// El DAW se puede hacer más chico que el editor. Sin limitarlo, la ventana
/// del plugin se sale de la pantalla y queda parcialmente behind del panel.
#[test]
fn el_cuadro_se_limita_al_tamanio_del_daw() {
    let frame = HostFrame::new(40, 50, 1200, 900).clamp_to(800, 600);
    assert_eq!((frame.width, frame.height), (800, 600));
    assert_eq!((frame.x, frame.y), (40, 50));

    // Un DAW de 1x1 no puede dejar el cuadro en negativo.
    let tiny = HostFrame::new(300, 300, 800, 600).clamp_to(1, 1);
    assert_eq!((tiny.x, tiny.y), (0, 0));
}

/// Con `override_redirect` el WM no gestiona la ventana, así que
/// `_NET_WM_STATE_ABOVE` no existe de hecho y el apilado lo tiene que hacer el
/// tick. Es el motivo por el que el keep-alive no se apaga en ese modo.
#[test]
fn una_ventana_sin_decoraciones_necesita_el_apilado_del_tick() {
    assert!(plan_hints(&WrapperConfig::default()).needs_raising());

    // Salvo que el usuario haya apagado `always_on_top` explícitamente.
    let config = WrapperConfig {
        always_on_top: false,
        ..WrapperConfig::default()
    };
    assert!(!plan_hints(&config).needs_raising());
}

#[test]
fn los_defaults_son_la_semantica_de_fruity_wrapper() {
    let plan = plan_hints(&WrapperConfig::with_parent(HostParentKind::Xlib(0x42)));

    assert!(
        plan.needs_raising(),
        "el editor tiene que quedarse arriba del DAW"
    );
    assert_eq!(plan.transient_for, Some(0x42));
    assert_eq!(plan.window_type, WrapperWindowType::Utility);
    assert!(
        plan.keyboard_input,
        "sin WM_HINTS.input hay WMs que nunca le dan el foco al editor"
    );
}

#[test]
fn en_wayland_la_ventana_queda_arriba_sin_poder_emparentarse() {
    let plan = plan_hints(&WrapperConfig::with_parent(HostParentKind::Wayland));

    assert_eq!(plan.transient_for, None, "no hay XID de Wayland");
    assert!(
        plan.needs_raising(),
        "el comportamiento se sostiene sobre _NET_WM_STATE_ABOVE"
    );
}

/// La regresión del input: el contenedor tiene que seleccionar puntero.
#[test]
fn el_contenedor_selecciona_los_eventos_de_entrada_del_plugin() {
    let mask = InputPolicy::default().mask;

    // Estos cuatro son exactamente los que faltaban y que hacen que los
    // clics sobre los controles del editor no hagan nada.
    assert!(mask.button_press, "ButtonPress: sin esto no hay click");
    assert!(mask.button_release, "ButtonRelease: sin esto el botón queda presionado");
    assert!(mask.pointer_motion, "MotionNotify: sin esto no hay arrastre de knob");
    assert!(mask.button_motion, "ButtonMotion: idem para el arrastre con botón bajo");
}

#[test]
fn la_activacion_del_editor_es_una_sola_vez_al_abrir() {
    // Reactivar periódicamente le robaría el foco al DAW mientras el usuario
    // arrastra un fader: es el defecto clásico de un envoltorio mal hecho.
    assert_eq!(
        InputPolicy::default().activation,
        InputActivation::OnOpen,
    );
    assert_eq!(InputPolicy::view_only().activation, InputActivation::Never);
}

#[test]
fn el_keep_alive_reafirma_el_stacking_sin_tocar_el_foco() {
    let plan = plan_hints(&WrapperConfig::default());
    let mut keep_alive = WrapperKeepAlive::for_plan(0, &plan);

    // Dispara...
    assert!(keep_alive.due(0));
    // ...y espera antes de repetir: 750ms es el intervalo por defecto.
    assert!(!keep_alive.due(WrapperKeepAlive::DEFAULT_INTERVAL_MS - 1));
    assert!(keep_alive.due(WrapperKeepAlive::DEFAULT_INTERVAL_MS));
}

#[test]
fn el_tick_por_defecto_es_de_60_hz() {
    // 60 y no 30: varios analizadores de plugin son de banda estrecha y a
    // 30 Hz el espectro se ve a saltos.
    assert_eq!(TickBudget::default().fps(), 60);
}

/// Un presupuesto de tick degenerado no puede colgar el hilo de la ventana.
#[test]
fn un_tick_degenerado_no_revienta_por_division_por_cero() {
    let budget = TickBudget::new(0, 0);

    assert!(budget.period().as_nanos() > 0);
    assert!(
        budget.feedback_due(1),
        "con `feedback_every` en 0 tiene que disparar siempre, no fallar"
    );
}

/// Los fallos del servidor no se reportan como cierres limpios.
///
/// `is_clean` decide si el rack pinta una línea roja de error. Confundir
/// "se perdió la conexión con el servidor X11" con "el usuario cerró la
/// ventana" esconde un problema real.
#[test]
fn perder_el_servidor_x11_no_es_un_cierre_limpio() {
    assert!(LoopExit::Requested.is_clean());
    assert!(LoopExit::UserClosed.is_clean());
    assert!(!LoopExit::Destroyed.is_clean());
    assert!(!LoopExit::ConnectionLost("connection reset".into()).is_clean());
}

// ───────────────────────────── El marco ─────────────────────────────

/// El editor tiene que tener su propio marco: título, bordes y botón de
/// cerrar.
///
/// Con `override_redirect` la ventana sale de la gestión del gestor de
/// ventanas del escritorio y con ella se va el marco entero. Sin esto, el
/// plugin queda pegado a la ventana como una pegatina, que es lo que se
/// reportó.
#[test]
fn el_editor_tiene_titulo_bordes_y_boton_de_cerrar() {
    let f = FrameMetrics::default();

    assert!(f.has_frame());
    assert!(f.titlebar_height > 0, "sin título no hay con qué arrastrar");
    assert!(f.border > 0, "sin bordes no se ve dónde termina el editor");
    assert!(f.close_button > 0, "sin botón de cerrar no hay forma de salir");
    assert!(f.resize_grip > 0, "sin tirador el tamaño queda fijo");
}

/// El hit-test reparte el puntero entre el marco y el editor.
///
/// El orden importa y es la diferencia entre que funcione o no: el botón de
/// cerrar vive **dentro** del titlebar, así que si el título ganara el clic de
/// cerrarla movería la ventana.
#[test]
fn el_titulo_arrastra_el_boton_cierra_y_el_resto_es_del_plugin() {
    let f = FrameMetrics::default();
    let (w, h) = (900u16, 600u16);

    assert_eq!(f.hit_test(400, 10, w, h), FrameRegion::TitleBar);
    assert_eq!(
        f.hit_test(w as i16 - 8, 10, w, h),
        FrameRegion::CloseButton,
        "el botón tiene que ganarle al título"
    );
    assert_eq!(
        f.hit_test(400, 300, w, h),
        FrameRegion::Inner,
        "el centro es del plugin, no del marco"
    );
}

/// Las esquinas y los bordes redimensionan; el título no.
#[test]
fn los_bordes_redimensionan_y_el_titulo_no() {
    let f = FrameMetrics::default();
    let (w, h) = (900u16, 600u16);

    assert_eq!(f.hit_test(1, 300, w, h), FrameRegion::Resize(ResizeDir::Left));
    assert_eq!(
        f.hit_test(w as i16 - 1, 300, w, h),
        FrameRegion::Resize(ResizeDir::Right)
    );
    assert_eq!(
        f.hit_test(450, h as i16 - 1, w, h),
        FrameRegion::Resize(ResizeDir::Bottom)
    );
    assert_eq!(
        f.hit_test(w as i16 - 1, h as i16 - 1, w, h),
        FrameRegion::Resize(ResizeDir::BottomRight)
    );
    // El borde superior es del título: si redimensionara, no se podría mover
    // la ventana arrastrando el título.
    assert_eq!(f.hit_test(400, 1, w, h), FrameRegion::TitleBar);
}

/// Redimensionar abajo-derecha no mueve el borde izquierdo.
///
/// Si también se moviera, la ventana "respiraría" y el editor quedaría
/// desalineado del puntero.
#[test]
fn redimensionar_una_esquina_ancla_el_borde_opuesto() {
    assert_eq!(ResizeDir::BottomRight.dx(10), -10);
    assert_eq!(ResizeDir::BottomRight.dy(10), -10);
    assert_eq!(ResizeDir::Bottom.dx(10), 0, "abajo no toca el ancho");
    assert_eq!(ResizeDir::Bottom.dy(10), -10);
}

/// El tamaño pedido por el plugin tiene que caber en el hueco del marco.
///
/// `show()` agranda la ventana por los insets para que el editor reciba lo que
/// pidió; si el inset no coincide con el rect interior, el plugin se
/// descuadra contra su propio layout.
#[test]
fn el_tamano_pedido_cabe_en_el_marco() {
    let f = FrameMetrics::default();

    // Pedidos 800x600 -> ventana 800+insets, interior = 800x600. Exacto.
    let window_w = 800 + f.horizontal_inset();
    let window_h = 600 + f.vertical_inset();
    let inner = f.inner_rect(window_w, window_h);
    assert_eq!((inner.width, inner.height), (800, 600));
}

/// Los plugins que ya traen su propio titlebar no se pueden envolver en otro.
#[test]
fn hay_un_modo_sin_marco_para_los_plugins_que_traen_el_suyo() {
    let f = FrameMetrics::frameless();
    assert!(!f.has_frame());
    let inner = f.inner_rect(800, 600);
    assert_eq!((inner.x, inner.y, inner.width, inner.height), (0, 0, 800, 600));
    assert_eq!((f.horizontal_inset(), f.vertical_inset()), (0, 0));
}

/// El tamaño mínimo tiene que dejarle un hueco usable al editor.
#[test]
fn el_minimo_no_deja_al_editor_sin_espacio() {
    let f = FrameMetrics::default();
    let (w, h) = f.min_size();
    assert!(f.inner_rect(w, h).is_usable());
}

/// El cursor tiene que acompañar a la zona.
#[test]
fn el_cursor_acompana_a_la_zona_del_marco() {
    let f = FrameMetrics::default();
    assert_eq!(f.cursor_for(FrameRegion::TitleBar), CursorHint::Default);
    assert_eq!(f.cursor_for(FrameRegion::CloseButton), CursorHint::Hand);
    assert_eq!(
        f.cursor_for(FrameRegion::Resize(ResizeDir::Left)),
        CursorHint::Resize(ResizeDir::Left)
    );
}


// ───────────────────── Por qué hay una ventana de contenido ─────────────────────

/// El plugin **no** puede dibujar encima del marco.
///
/// Dos estilos de editor conviven en VST3/CLAP:
///
/// - Los que crean su propia ventana hija (JUCE, Qt) y se ajustan a ella.
/// - Los que dibujan directamente sobre la ventana que les pasó el host.
///
/// En el segundo caso, darle el contenedor hace que el editor pinte encima
/// del título y de los bordes: el marco deja de existir. Por eso el host le
/// pasa su **propia** ventana de contenido, ya colocada en el hueco interior;
/// el contenedor queda siendo sólo el marco.
///
/// Es el motivo de que el título de MonaChroma no apareciera, y es
/// independiente de si el plugin crea o no una ventana hija.
#[test]
fn el_plugin_dibuja_en_una_ventana_propia_nunca_en_el_marco() {
    // Verificado por construcción en `HikaruWrapperWindow::raw_handle`: la
    // ventana de contenido es hija del contenedor y ocupa sólo el hueco
    // interior. Este test fija el invariante geométrico del que depende.
    let f = FrameMetrics::default();
    let (w, h) = (900u16, 600u16);
    let inner = f.inner_rect(w, h);

    // El contenido tiene que estar **dentro** del marco, y el marco tiene que
    // seguir teniendo grosor arriba y a los lados.
    assert!(inner.y > 0, "el contenido no puede empezar en el borde");
    assert!(inner.x > 0);
    assert!(inner.y + (inner.height as i16) < h as i16, "falta borde abajo");
    assert!(inner.x + (inner.width as i16) < w as i16, "falta borde a la derecha");
    assert!(inner.is_usable());
}

/// El título no puede tragarse el borde superior.
///
/// Es lo que hace que la ventana se pueda mover arrastrándola. Si el borde
/// superior redimensionara, la franja de título quedaría partida y el clic de
/// mover sería un resize accidental.
#[test]
fn el_titulo_no_redimensiona_la_ventana() {
    let f = FrameMetrics::default();
    let (w, h) = (900u16, 600u16);
    for x in [0, 200, 450, 700] {
        assert_eq!(
            f.hit_test(x, 1, w, h),
            FrameRegion::TitleBar,
            "arriba, x={x}: tiene que ser título, no resize"
        );
    }
}

/// Cerrar y arrastrar no pueden confundirse.
///
/// El botón de cerrar vive **dentro** del titlebar. Si el título ganara el
/// hit-test, el clic de cerrar movería la ventana en vez de cerrarla.
#[test]
fn el_boton_de_cerrar_no_arrastra_la_ventana() {
    let f = FrameMetrics::default();
    let (w, h) = (900u16, 600u16);
    let close = f.hit_test(w as i16 - 8, 10, w, h);
    assert_eq!(close, FrameRegion::CloseButton);
    assert!(close.is_chrome());
    assert_ne!(close, FrameRegion::TitleBar);
}

/// El marco tiene que caber en la ventana mínima.
///
/// Si no, el mínimo dejaría al editor con un hueco de cero y se vería una
/// franja de marco donde debería estar el plugin.
#[test]
fn el_minimo_respeta_el_marco() {
    let f = FrameMetrics::default();
    let (w, h) = f.min_size();
    let inner = f.inner_rect(w, h);
    assert!(inner.is_usable(), "min {:?} deja el editor sin hueco", (w, h));
    assert!(inner.height > 0 && inner.width > 0);
}
