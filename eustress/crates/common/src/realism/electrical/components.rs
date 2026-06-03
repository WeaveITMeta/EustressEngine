//! ECS components for circuit simulation.
//! Any entity can participate in a circuit by adding these components.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

// ── Node state ────────────────────────────────────────────────────

/// Node voltage component — attach to any entity that is a circuit node.
#[derive(Component, Reflect, Clone, Debug, Default, Serialize, Deserialize)]
#[reflect(Component)]
pub struct ElectricalNode {
    /// Voltage in volts relative to ground.
    pub voltage: f32,
    /// Net current leaving the node in amperes.
    pub current_out: f32,
}

// ── Passive elements ─────────────────────────────────────────────

#[derive(Component, Reflect, Clone, Debug, Serialize, Deserialize)]
#[reflect(Component)]
pub struct Resistor {
    pub resistance_ohms: f32,
    pub power_rating_w: f32,
    /// Computed: I = V / R
    pub current: f32,
    /// Computed: P = I² · R
    pub power_dissipated: f32,
}

impl Default for Resistor {
    fn default() -> Self {
        Self {
            resistance_ohms: 1000.0, // 1 kΩ — a safe general-purpose value
            power_rating_w: 0.25,    // 1/4 W carbon-film standard
            current: 0.0,
            power_dissipated: 0.0,
        }
    }
}

#[derive(Component, Reflect, Clone, Debug, Serialize, Deserialize)]
#[reflect(Component)]
pub struct Capacitor {
    pub capacitance_farads: f32,
    /// State variable: voltage across the capacitor in volts.
    pub voltage: f32,
    /// Computed: I = C · dV/dt
    pub current: f32,
    /// Computed: E = ½ · C · V²
    pub stored_energy_joules: f32,
    /// Rated max voltage; exceeding triggers a warning.
    pub max_voltage: f32,
}

impl Default for Capacitor {
    fn default() -> Self {
        Self {
            capacitance_farads: 100e-6, // 100 µF electrolytic
            voltage: 0.0,
            current: 0.0,
            stored_energy_joules: 0.0,
            max_voltage: 50.0, // 50 V rated
        }
    }
}

#[derive(Component, Reflect, Clone, Debug, Serialize, Deserialize)]
#[reflect(Component)]
pub struct Inductor {
    pub inductance_henries: f32,
    /// State variable: current through the inductor in amperes.
    pub current: f32,
    /// Computed: V = L · dI/dt
    pub voltage: f32,
    /// Computed: E = ½ · L · I²
    pub stored_energy_joules: f32,
    /// Parasitic series (DC) resistance in ohms.
    pub resistance_ohms: f32,
}

impl Default for Inductor {
    fn default() -> Self {
        Self {
            inductance_henries: 1e-3, // 1 mH
            current: 0.0,
            voltage: 0.0,
            stored_energy_joules: 0.0,
            resistance_ohms: 0.1, // 100 mΩ DCR — typical for a small power inductor
        }
    }
}

// ── Sources ───────────────────────────────────────────────────────

#[derive(Component, Reflect, Clone, Debug, Serialize, Deserialize)]
#[reflect(Component)]
pub struct VoltageSource {
    /// Ideal open-circuit voltage in volts.
    pub voltage: f32,
    /// Series internal resistance in ohms.
    pub internal_resistance: f32,
    /// Computed: current delivered to the circuit.
    pub current: f32,
    pub enabled: bool,
}

impl Default for VoltageSource {
    fn default() -> Self {
        Self {
            voltage: 12.0,            // 12 V — common automotive / lab supply
            internal_resistance: 0.1, // 100 mΩ — typical bench supply output impedance
            current: 0.0,
            enabled: true,
        }
    }
}

#[derive(Component, Reflect, Clone, Debug, Serialize, Deserialize)]
#[reflect(Component)]
pub struct CurrentSource {
    /// Ideal short-circuit current in amperes.
    pub current: f32,
    /// Parallel internal conductance in siemens (1 / parallel resistance).
    pub internal_conductance: f32,
    /// Computed: voltage appearing across the source terminals.
    pub voltage: f32,
    pub enabled: bool,
}

impl Default for CurrentSource {
    fn default() -> Self {
        Self {
            current: 1.0,                 // 1 A
            internal_conductance: 1e-6,   // 1 MΩ parallel — nearly ideal
            voltage: 0.0,
            enabled: true,
        }
    }
}

// ── Semiconductor (simplified) ────────────────────────────────────

#[derive(Component, Reflect, Clone, Debug, Serialize, Deserialize)]
#[reflect(Component)]
pub struct Diode {
    /// Forward voltage drop in volts.
    pub forward_voltage: f32,
    /// Reverse breakdown voltage (magnitude) in volts.
    pub reverse_breakdown: f32,
    /// Computed via Shockley equation; signed.
    pub current: f32,
    pub is_conducting: bool,
}

impl Default for Diode {
    fn default() -> Self {
        Self::silicon()
    }
}

impl Diode {
    pub fn silicon() -> Self {
        Self {
            forward_voltage: 0.7,
            reverse_breakdown: 50.0,
            current: 0.0,
            is_conducting: false,
        }
    }

    pub fn schottky() -> Self {
        Self {
            forward_voltage: 0.3,
            reverse_breakdown: 40.0,
            current: 0.0,
            is_conducting: false,
        }
    }

    pub fn led_red() -> Self {
        Self {
            forward_voltage: 1.8,
            reverse_breakdown: 5.0,
            current: 0.0,
            is_conducting: false,
        }
    }

    pub fn led_blue() -> Self {
        Self {
            forward_voltage: 3.2,
            reverse_breakdown: 5.0,
            current: 0.0,
            is_conducting: false,
        }
    }

    /// Zener diode with a specified breakdown voltage.
    pub fn zener(vz: f32) -> Self {
        Self {
            forward_voltage: 0.7,
            reverse_breakdown: vz,
            current: 0.0,
            is_conducting: false,
        }
    }

    /// Compute whether this diode conducts given the anode-to-cathode voltage.
    /// Returns the approximate current using a piecewise-linear model.
    pub fn compute_current(&mut self, v_ak: f32) -> f32 {
        if v_ak >= self.forward_voltage {
            // Forward conducting: V_ak - V_f across a small series resistance (1 Ω model)
            let i = (v_ak - self.forward_voltage).max(0.0) / 1.0;
            self.is_conducting = true;
            self.current = i;
            i
        } else if v_ak <= -self.reverse_breakdown {
            // Breakdown (Zener / avalanche): model as 1 Ω source
            let i = -((-v_ak - self.reverse_breakdown).max(0.0) / 1.0);
            self.is_conducting = true;
            self.current = i;
            i
        } else {
            self.is_conducting = false;
            self.current = 0.0;
            0.0
        }
    }
}

// ── Circuit edge ──────────────────────────────────────────────────

/// Connects two node entities via a circuit element.
/// Attach this component to a dedicated edge entity alongside the element entity refs.
#[derive(Component, Reflect, Clone, Debug, Serialize, Deserialize)]
#[reflect(Component)]
pub struct CircuitBranch {
    pub node_a: Entity,
    pub node_b: Entity,
    pub element: CircuitElement,
    /// Current from node_a to node_b in amperes (signed, computed each tick).
    pub current: f32,
}

impl Default for CircuitBranch {
    fn default() -> Self {
        Self {
            node_a: Entity::PLACEHOLDER,
            node_b: Entity::PLACEHOLDER,
            element: CircuitElement::Wire,
            current: 0.0,
        }
    }
}

#[derive(Reflect, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum CircuitElement {
    /// Ideal resistor; value in ohms.
    Resistor(f32),
    /// Ideal capacitor; value in farads.
    Capacitor(f32),
    /// Ideal inductor; value in henries.
    Inductor(f32),
    /// Perfect short-circuit wire (0 Ω).
    Wire,
    /// Ideal voltage source; value in volts (node_a is + terminal).
    VoltageSource(f32),
    /// Ideal current source; value in amperes (positive = from node_a to node_b).
    CurrentSource(f32),
}

impl CircuitElement {
    /// Returns the DC (steady-state) conductance of the element in siemens.
    /// Capacitors → open (0 S), Inductors → short (very large S).
    pub fn dc_conductance(&self) -> f32 {
        match self {
            CircuitElement::Resistor(r) => {
                if *r > f32::EPSILON {
                    1.0 / r
                } else {
                    f32::MAX
                }
            }
            CircuitElement::Wire => f32::MAX,
            CircuitElement::Capacitor(_) => 0.0,
            CircuitElement::Inductor(_) => f32::MAX,
            CircuitElement::VoltageSource(_) => f32::MAX,
            CircuitElement::CurrentSource(_) => 0.0,
        }
    }
}

// ── Power summary ──────────────────────────────────────────────────

/// Bus-level power summary — attach to the circuit root entity.
#[derive(Component, Reflect, Clone, Debug, Default, Serialize, Deserialize)]
#[reflect(Component)]
pub struct PowerBus {
    /// Total power generated by all sources on this bus in watts.
    pub total_power_generated: f32,
    /// Total power consumed by all loads on this bus in watts.
    pub total_power_consumed: f32,
    /// Nominal bus voltage set by the source in volts.
    pub bus_voltage: f32,
    /// 0 = DC, 50 = European AC, 60 = North-American AC.
    pub frequency_hz: f32,
}

impl PowerBus {
    /// A standard 12 V DC bus (automotive / embedded systems).
    pub fn dc_12v() -> Self {
        Self {
            bus_voltage: 12.0,
            frequency_hz: 0.0,
            ..Default::default()
        }
    }

    /// A standard 5 V DC bus (USB / logic power rail).
    pub fn dc_5v() -> Self {
        Self {
            bus_voltage: 5.0,
            frequency_hz: 0.0,
            ..Default::default()
        }
    }

    /// A 48 V DC bus (modern automotive high-voltage or server rack).
    pub fn dc_48v() -> Self {
        Self {
            bus_voltage: 48.0,
            frequency_hz: 0.0,
            ..Default::default()
        }
    }

    /// 120 V / 60 Hz North-American mains.
    pub fn ac_120v_60hz() -> Self {
        Self {
            bus_voltage: 120.0,
            frequency_hz: 60.0,
            ..Default::default()
        }
    }

    /// 230 V / 50 Hz European mains.
    pub fn ac_230v_50hz() -> Self {
        Self {
            bus_voltage: 230.0,
            frequency_hz: 50.0,
            ..Default::default()
        }
    }

    /// Net balance: positive means the bus is generating more than consuming (surplus).
    pub fn power_balance(&self) -> f32 {
        self.total_power_generated - self.total_power_consumed
    }

    /// True when the bus is within 5 % of balance (reasonable steady-state).
    pub fn is_balanced(&self) -> bool {
        self.total_power_generated > f32::EPSILON
            && (self.power_balance().abs() / self.total_power_generated) < 0.05
    }
}
