use crate::errors::AkrepError;
use crate::models::FirewallProto;
use std::net::{Ipv4Addr, Ipv6Addr};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IpFamily {
    Ipv4,
    Ipv6,
}

impl IpFamily {
    pub fn as_firewalld_str(self) -> &'static str {
        match self {
            Self::Ipv4 => "ipv4",
            Self::Ipv6 => "ipv6",
        }
    }
}

pub fn parse_and_validate_cidr(cidr: &str) -> Result<(IpFamily, String), AkrepError> {
    if cidr.is_empty() || cidr.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(AkrepError::InvalidInput(
            "CIDR must not be empty or contain whitespace".to_string(),
        ));
    }
    let Some((addr_part, prefix_part)) = cidr.split_once('/') else {
        return Err(AkrepError::InvalidInput(format!(
            "CIDR '{}' must be in <address>/<prefix> format",
            cidr
        )));
    };
    if prefix_part.is_empty()
        || prefix_part.contains('/')
        || !prefix_part.bytes().all(|b| b.is_ascii_digit())
        || (prefix_part.len() > 1 && prefix_part.starts_with('0'))
    {
        return Err(AkrepError::InvalidInput(format!(
            "Invalid CIDR prefix length in '{}'",
            cidr
        )));
    }
    let prefix: u8 = prefix_part
        .parse()
        .map_err(|_| AkrepError::InvalidInput(format!("CIDR prefix out of range in '{}'", cidr)))?;

    if let Ok(v4) = addr_part.parse::<Ipv4Addr>() {
        if prefix == 0 || prefix > 32 {
            return Err(AkrepError::InvalidInput(format!(
                "IPv4 CIDR prefix must be in 1..=32 (got {})",
                prefix
            )));
        }
        return Ok((IpFamily::Ipv4, format!("{}/{}", v4, prefix)));
    }

    if let Ok(v6) = addr_part.parse::<Ipv6Addr>() {
        if prefix == 0 || prefix > 128 {
            return Err(AkrepError::InvalidInput(format!(
                "IPv6 CIDR prefix must be in 1..=128 (got {})",
                prefix
            )));
        }
        return Ok((IpFamily::Ipv6, format!("{}/{}", v6, prefix)));
    }

    Err(AkrepError::InvalidInput(format!(
        "Invalid IPv4/IPv6 address in CIDR '{}'",
        cidr
    )))
}

pub fn validate_port_spec(port: &str) -> Result<(), AkrepError> {
    if port.is_empty() {
        return Err(AkrepError::InvalidInput(
            "Port specification cannot be empty".to_string(),
        ));
    }

    let parse_single = |part: &str| -> Result<u16, AkrepError> {
        if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) || part.starts_with('0') {
            return Err(AkrepError::InvalidInput(format!(
                "Invalid port number '{}'",
                part
            )));
        }
        let num: u32 = part.parse().map_err(|_| {
            AkrepError::InvalidInput(format!("Port '{}' out of range 1..=65535", part))
        })?;
        if !(1..=65535).contains(&num) {
            return Err(AkrepError::InvalidInput(format!(
                "Port {} must be within 1..=65535",
                num
            )));
        }
        Ok(num as u16)
    };

    if let Some((start_s, end_s)) = port.split_once('-') {
        if end_s.contains('-') {
            return Err(AkrepError::InvalidInput(format!(
                "Malformed port range '{}'",
                port
            )));
        }
        let start = parse_single(start_s)?;
        let end = parse_single(end_s)?;
        if start > end {
            return Err(AkrepError::InvalidInput(format!(
                "Port range start {} exceeds end {}",
                start, end
            )));
        }
        Ok(())
    } else {
        let _ = parse_single(port)?;
        Ok(())
    }
}

pub fn validate_service_name(service: &str) -> Result<(), AkrepError> {
    if service.is_empty() || service.len() > 64 {
        return Err(AkrepError::InvalidInput(
            "Firewall service name must be 1..=64 characters".to_string(),
        ));
    }
    if !service
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
    {
        return Err(AkrepError::InvalidInput(format!(
            "Invalid firewall service name '{}' (expected ^[a-z0-9_-]+$)",
            service
        )));
    }
    Ok(())
}

pub fn build_cidr_port_rich_rule(
    cidr: &str,
    port: &str,
    proto: FirewallProto,
) -> Result<String, AkrepError> {
    let (family, normalized_cidr) = parse_and_validate_cidr(cidr)?;
    validate_port_spec(port)?;
    Ok(format!(
        "rule family=\"{}\" source address=\"{}\" port port=\"{}\" protocol=\"{}\" accept",
        family.as_firewalld_str(),
        normalized_cidr,
        port,
        proto.as_str()
    ))
}

pub fn build_cidr_service_rich_rule(cidr: &str, service: &str) -> Result<String, AkrepError> {
    let (family, normalized_cidr) = parse_and_validate_cidr(cidr)?;
    validate_service_name(service)?;
    Ok(format!(
        "rule family=\"{}\" source address=\"{}\" service name=\"{}\" accept",
        family.as_firewalld_str(),
        normalized_cidr,
        service
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_cidr_port_and_service_rich_rules_deterministic() {
        let r1 = build_cidr_port_rich_rule("192.168.10.0/24", "8443", FirewallProto::Tcp).unwrap();
        assert_eq!(
            r1,
            "rule family=\"ipv4\" source address=\"192.168.10.0/24\" port port=\"8443\" protocol=\"tcp\" accept"
        );

        let r2 =
            build_cidr_port_rich_rule("2001:0db8::/32", "60000-60010", FirewallProto::Udp).unwrap();
        assert_eq!(
            r2,
            "rule family=\"ipv6\" source address=\"2001:db8::/32\" port port=\"60000-60010\" protocol=\"udp\" accept"
        );

        let r3 = build_cidr_service_rich_rule("10.0.0.5/32", "cockpit").unwrap();
        assert_eq!(
            r3,
            "rule family=\"ipv4\" source address=\"10.0.0.5/32\" service name=\"cockpit\" accept"
        );
    }

    #[test]
    fn test_rich_rule_rejects_invalid_cidr_ports_and_services() {
        assert!(build_cidr_port_rich_rule("0.0.0.0/0", "80", FirewallProto::Tcp).is_err());
        assert!(build_cidr_port_rich_rule("192.168.1.0/33", "80", FirewallProto::Tcp).is_err());
        assert!(build_cidr_port_rich_rule("192.168.1.0/24", "0", FirewallProto::Tcp).is_err());
        assert!(build_cidr_port_rich_rule("192.168.1.0/24", "70000", FirewallProto::Tcp).is_err());
        assert!(
            build_cidr_port_rich_rule("192.168.1.0/24", "9000-8000", FirewallProto::Tcp).is_err()
        );
        assert!(build_cidr_service_rich_rule("192.168.1.0/24", "SSH").is_err());
        assert!(build_cidr_service_rich_rule("192.168.1.0/24", "ssh; accept").is_err());
    }
}
