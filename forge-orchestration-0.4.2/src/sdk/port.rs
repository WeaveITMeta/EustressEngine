//! Port allocation for Forge workloads
//!
//! Supports TCP, UDP, and combined port allocation for game servers and services.

use super::{SdkError, SdkResult};
use std::collections::HashMap;
use std::net::{SocketAddr, TcpListener, UdpSocket};
use std::ops::Range;
use std::sync::Mutex;
use tracing::{debug, info};

lazy_static::lazy_static! {
    static ref ALLOCATED_PORTS: Mutex<HashMap<u16, Protocol>> = Mutex::new(HashMap::new());
}

/// Protocol type for port allocation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Protocol {
    /// TCP only
    Tcp,
    /// UDP only
    Udp,
    /// Both TCP and UDP on the same port (common for game servers)
    Both,
}

/// Represents an allocated port with its protocol
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AllocatedPort {
    /// Port number
    pub port: u16,
    /// Protocol(s) allocated
    pub protocol: Protocol,
}

impl AllocatedPort {
    /// Create a new allocated port
    pub fn new(port: u16, protocol: Protocol) -> Self {
        Self { port, protocol }
    }
}

// =============================================================================
// TCP Port Allocation (backward compatible)
// =============================================================================

/// Allocate a TCP port from the given range
///
/// Attempts to bind to ports in the range until one succeeds.
pub fn allocate_port(range: Range<u16>) -> SdkResult<u16> {
    allocate_port_with_protocol(range, Protocol::Tcp).map(|ap| ap.port)
}

/// Allocate a specific TCP port
pub fn allocate_specific_port(port: u16) -> SdkResult<u16> {
    allocate_specific_port_with_protocol(port, Protocol::Tcp).map(|ap| ap.port)
}

// =============================================================================
// UDP Port Allocation (for game servers)
// =============================================================================

/// Allocate a UDP port from the given range
///
/// Game servers typically need UDP for real-time communication.
pub fn allocate_udp_port(range: Range<u16>) -> SdkResult<AllocatedPort> {
    allocate_port_with_protocol(range, Protocol::Udp)
}

/// Allocate a specific UDP port
pub fn allocate_specific_udp_port(port: u16) -> SdkResult<AllocatedPort> {
    allocate_specific_port_with_protocol(port, Protocol::Udp)
}

// =============================================================================
// Combined TCP+UDP Port Allocation (for game servers)
// =============================================================================

/// Allocate both TCP and UDP on the same port
///
/// Common pattern for game servers that need both protocols on the same port.
pub fn allocate_game_port(range: Range<u16>) -> SdkResult<AllocatedPort> {
    allocate_port_with_protocol(range, Protocol::Both)
}

/// Allocate a specific port for both TCP and UDP
pub fn allocate_specific_game_port(port: u16) -> SdkResult<AllocatedPort> {
    allocate_specific_port_with_protocol(port, Protocol::Both)
}

// =============================================================================
// Core Implementation
// =============================================================================

/// Allocate a port with specified protocol
pub fn allocate_port_with_protocol(range: Range<u16>, protocol: Protocol) -> SdkResult<AllocatedPort> {
    let mut allocated = ALLOCATED_PORTS
        .lock()
        .map_err(|_| SdkError::port("Failed to lock port allocator"))?;

    for port in range {
        if allocated.contains_key(&port) {
            continue;
        }

        if try_bind_port(port, protocol)? {
            allocated.insert(port, protocol);
            info!(port = port, protocol = ?protocol, "Port allocated");
            return Ok(AllocatedPort::new(port, protocol));
        } else {
            debug!(port = port, "Port in use, trying next");
        }
    }

    Err(SdkError::port("No available ports in range"))
}

/// Allocate a specific port with specified protocol
pub fn allocate_specific_port_with_protocol(port: u16, protocol: Protocol) -> SdkResult<AllocatedPort> {
    let mut allocated = ALLOCATED_PORTS
        .lock()
        .map_err(|_| SdkError::port("Failed to lock port allocator"))?;

    if allocated.contains_key(&port) {
        return Err(SdkError::port(format!("Port {} already allocated", port)));
    }

    if try_bind_port(port, protocol)? {
        allocated.insert(port, protocol);
        info!(port = port, protocol = ?protocol, "Specific port allocated");
        Ok(AllocatedPort::new(port, protocol))
    } else {
        Err(SdkError::port(format!("Port {} unavailable", port)))
    }
}

/// Try to bind a port with the specified protocol
fn try_bind_port(port: u16, protocol: Protocol) -> SdkResult<bool> {
    let addr: SocketAddr = ([0, 0, 0, 0], port).into();

    match protocol {
        Protocol::Tcp => {
            match TcpListener::bind(addr) {
                Ok(_listener) => Ok(true),
                Err(_) => Ok(false),
            }
        }
        Protocol::Udp => {
            match UdpSocket::bind(addr) {
                Ok(_socket) => Ok(true),
                Err(_) => Ok(false),
            }
        }
        Protocol::Both => {
            // Must be able to bind both TCP and UDP
            let tcp_ok = TcpListener::bind(addr).is_ok();
            let udp_ok = UdpSocket::bind(addr).is_ok();
            Ok(tcp_ok && udp_ok)
        }
    }
}

/// Release an allocated port
pub fn release_port(port: u16) -> SdkResult<()> {
    let mut allocated = ALLOCATED_PORTS
        .lock()
        .map_err(|_| SdkError::port("Failed to lock port allocator"))?;

    if allocated.remove(&port).is_some() {
        info!(port = port, "Port released");
    } else {
        debug!(port = port, "Port was not allocated");
    }

    Ok(())
}

/// Get all currently allocated ports
pub fn allocated_ports() -> Vec<u16> {
    match ALLOCATED_PORTS.lock() {
        Ok(guard) => guard.keys().copied().collect(),
        Err(_) => Vec::new(),
    }
}

/// Get all allocated ports with their protocols
pub fn allocated_ports_detailed() -> Vec<AllocatedPort> {
    match ALLOCATED_PORTS.lock() {
        Ok(guard) => guard.iter().map(|(&port, &protocol)| AllocatedPort::new(port, protocol)).collect(),
        Err(_) => Vec::new(),
    }
}

/// Check if a TCP port is available
pub fn is_port_available(port: u16) -> bool {
    is_port_available_for_protocol(port, Protocol::Tcp)
}

/// Check if a UDP port is available
pub fn is_udp_port_available(port: u16) -> bool {
    is_port_available_for_protocol(port, Protocol::Udp)
}

/// Check if a port is available for the specified protocol
pub fn is_port_available_for_protocol(port: u16, protocol: Protocol) -> bool {
    try_bind_port(port, protocol).unwrap_or(false)
}

// =============================================================================
// Port Allocator Struct (for custom configurations)
// =============================================================================

/// Port allocator with custom configuration
pub struct PortAllocator {
    range: Range<u16>,
    protocol: Protocol,
    allocated: HashMap<u16, Protocol>,
}

impl PortAllocator {
    /// Create a new TCP port allocator with the given range
    pub fn new(range: Range<u16>) -> Self {
        Self {
            range,
            protocol: Protocol::Tcp,
            allocated: HashMap::new(),
        }
    }

    /// Create a new UDP port allocator
    pub fn udp(range: Range<u16>) -> Self {
        Self {
            range,
            protocol: Protocol::Udp,
            allocated: HashMap::new(),
        }
    }

    /// Create a new game server port allocator (TCP+UDP)
    pub fn game(range: Range<u16>) -> Self {
        Self {
            range,
            protocol: Protocol::Both,
            allocated: HashMap::new(),
        }
    }

    /// Allocate the next available port
    pub fn allocate(&mut self) -> SdkResult<AllocatedPort> {
        for port in self.range.clone() {
            if self.allocated.contains_key(&port) {
                continue;
            }

            if is_port_available_for_protocol(port, self.protocol) {
                self.allocated.insert(port, self.protocol);
                return Ok(AllocatedPort::new(port, self.protocol));
            }
        }

        Err(SdkError::port("No available ports in range"))
    }

    /// Release a port
    pub fn release(&mut self, port: u16) {
        self.allocated.remove(&port);
    }

    /// Get allocated port count
    pub fn allocated_count(&self) -> usize {
        self.allocated.len()
    }

    /// Get all allocated ports
    pub fn allocated(&self) -> Vec<AllocatedPort> {
        self.allocated.iter().map(|(&port, &protocol)| AllocatedPort::new(port, protocol)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tcp_port_allocation() {
        let port = allocate_port(49152..49200).unwrap();
        assert!(port >= 49152 && port < 49200);
        release_port(port).unwrap();
    }

    #[test]
    fn test_udp_port_allocation() {
        let allocated = allocate_udp_port(49200..49250).unwrap();
        assert!(allocated.port >= 49200 && allocated.port < 49250);
        assert_eq!(allocated.protocol, Protocol::Udp);
        release_port(allocated.port).unwrap();
    }

    #[test]
    fn test_game_port_allocation() {
        let allocated = allocate_game_port(49250..49300).unwrap();
        assert!(allocated.port >= 49250 && allocated.port < 49300);
        assert_eq!(allocated.protocol, Protocol::Both);
        release_port(allocated.port).unwrap();
    }

    #[test]
    fn test_port_allocator_game() {
        let mut allocator = PortAllocator::game(49300..49350);
        let port1 = allocator.allocate().unwrap();
        let port2 = allocator.allocate().unwrap();
        assert_ne!(port1.port, port2.port);
        assert_eq!(allocator.allocated_count(), 2);
        allocator.release(port1.port);
        assert_eq!(allocator.allocated_count(), 1);
    }
}
