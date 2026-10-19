// Copyright (C) Hikaru Corporation - 2026
// GNU Lesser General Public License v3
// crates/hikaru_plugin_host/src/hikaru_wrapper/keep_alive.rs

//! Recordatorio periódico de que la ventana tiene que seguir arriba.

/// Re-afirma el estado "estoy arriba" cada tanto.
///
/// # Por qué hace falta
///
/// `_NET_WM_STATE_ABOVE` se aplica **una vez**, antes del `map`. A partir de
/// ahí el estado lo lleva el WM, y cada vez que el usuario clickea el DAW el
/// gestor vuelve a subir su ventana y deja la del plugin detrás. El hint no se
/// re-evalúa solo.
///
/// La cura no es vigilar el stacking (que en X11 exige recorrer el árbol de
/// ventanas y comparar niveles, frágil bajo WM reparentadores) sino re-afirmar
/// el estado cada tanto: `ConfigureWindow` con `Above` sobre una ventana que ya
/// está arriba es un no-op visual, y sobre una que quedó atrás la devuelve a su
/// sitio. El coste es un viaje al servidor por ventana abierta cada
/// [`Self::DEFAULT_INTERVAL_MS`], despreciable.
///
/// Deliberadamente **no** manda `_NET_ACTIVE_WINDOW`: eso además de subir la
/// ventana enfocaría el editor, y el usuario no podría seguir clickeando el
/// DAW. Ver la nota de módulo.
#[derive(Debug, Clone)]
pub struct WrapperKeepAlive {
    interval_ms: u64,
    next_due_ms: u64,
    enabled: bool,
}

impl WrapperKeepAlive {
    /// Cada cuánto re-afirmar el estado. 750ms es un buen punto medio: a 250ms
    /// el servidor se nota, y a 750ms la ventana tarda como mucho un instante
    /// en volver, que es imperceptible al volver el cursor al editor.
    pub const DEFAULT_INTERVAL_MS: u64 = 750;

    pub fn new(now_ms: u64) -> Self {
        Self {
            interval_ms: Self::DEFAULT_INTERVAL_MS,
            next_due_ms: now_ms,
            enabled: true,
        }
    }

    /// Un keep-alive que no hace nada.
    ///
    /// Para cuando el plan de hints no pide `ABOVE`: no tiene sentido gastar
    /// un viaje al servidor cada 750ms si nadie pidió estar arriba.
    pub fn disabled() -> Self {
        Self {
            interval_ms: Self::DEFAULT_INTERVAL_MS,
            next_due_ms: u64::MAX,
            enabled: false,
        }
    }

    /// Keep-alive según el plan: activo si el plan pide `ABOVE`.
    pub fn for_plan(now_ms: u64, plan: &super::WrapperHintPlan) -> Self {
        if plan.needs_raising() {
            Self::new(now_ms)
        } else {
            Self::disabled()
        }
    }

    /// Cambia el intervalo (en ms). Un valor 0 desactiva.
    pub fn set_interval_ms(&mut self, interval_ms: u64) {
        if interval_ms == 0 {
            self.enabled = false;
            self.next_due_ms = u64::MAX;
        } else {
            self.enabled = true;
            self.interval_ms = interval_ms;
        }
    }

    /// Si toca re-afirmar el estado en este instante, y agenda el próximo.
    pub fn due(&mut self, now_ms: u64) -> bool {
        if !self.enabled || now_ms < self.next_due_ms {
            return false;
        }
        self.next_due_ms = now_ms.saturating_add(self.interval_ms);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fires_immediately_then_on_interval() {
        let mut ka = WrapperKeepAlive::new(1_000);
        assert!(ka.due(1_000), "el primer chequeo es inmediato");
        assert!(!ka.due(1_001), "dentro del intervalo no repite");
        assert!(!ka.due(1_749));
        assert!(ka.due(1_750), "justo al completion del intervalo");
    }

    #[test]
    fn disabled_never_fires() {
        let mut ka = WrapperKeepAlive::disabled();
        assert!(!ka.due(0));
        assert!(!ka.due(u64::MAX));
    }

    #[test]
    fn follows_the_plan() {
        let plan = super::super::plan_hints(&super::super::WrapperConfig::default());
        assert!(WrapperKeepAlive::for_plan(0, &plan).due(0));

        // Hay que apagar `always_on_top` explícitamente: con el default
        // `Owned` la ventana es override-redirect y el apilado lo hace el tick
        // aunque no haya hint de WM del que ocuparse.
        let without_above = super::super::WrapperConfig {
            always_on_top: false,
            ..Default::default()
        };
        let plan = super::super::plan_hints(&without_above);
        let mut ka = WrapperKeepAlive::for_plan(0, &plan);
        assert!(!ka.due(0), "sin ABOVE no se gasta un viaje al servidor");
    }

    #[test]
    fn survives_a_clock_that_does_not_move_forward() {
        // `now` clavado (o un reloj que retrocedió): no se debe entrar en un
        // bucle que repita el chequeo para siempre.
        let mut ka = WrapperKeepAlive::new(500);
        assert!(ka.due(500));
        assert!(!ka.due(500));
        assert!(!ka.due(400));
        assert!(!ka.due(0));
    }

    #[test]
    fn interval_zero_disables() {
        let mut ka = WrapperKeepAlive::new(0);
        ka.set_interval_ms(0);
        assert!(!ka.due(0));
        assert!(!ka.due(10_000));
    }
}