//! Alogorithm and types for [`address validation`].
//!
//! [`address validation`]: https://datatracker.ietf.org/doc/html/rfc9000#name-address-validation

use std::{
    net::SocketAddr,
    time::{Duration, Instant, SystemTime},
};

use boring::sha::Sha256;
use quiche::{ConnectionId, Header, RecvInfo, SendInfo};

use crate::{Error, Poll, Result, utils::random_conn_id};

/// Address validation trait.
pub trait AddressValidator {
    /// Create a retry-token.
    fn mint_retry_token(
        &self,
        scid: &ConnectionId<'_>,
        dcid: &ConnectionId<'_>,
        new_scid: &ConnectionId<'_>,
        src: &SocketAddr,
    ) -> Result<Vec<u8>>;

    /// Validate the source address.
    fn validate_address<'a>(
        &self,
        scid: &ConnectionId<'_>,
        dcid: &ConnectionId<'_>,
        src: &SocketAddr,
        token: &'a [u8],
    ) -> Option<ConnectionId<'a>>;
}

/// A default implementation for [`AddressValidator`]
pub struct SimpleAddressValidator([u8; 20], Duration);

impl SimpleAddressValidator {
    /// Create a new `SimpleAddressValidator` instance with token expiration interval.
    pub fn new(expiration_interval: Duration) -> Self {
        let mut seed = [0; 20];
        boring::rand::rand_bytes(&mut seed).unwrap();
        Self(seed, expiration_interval)
    }
}

#[allow(unused)]
impl AddressValidator for SimpleAddressValidator {
    fn mint_retry_token(
        &self,
        _scid: &ConnectionId<'_>,
        dcid: &ConnectionId<'_>,
        new_scid: &ConnectionId<'_>,
        src: &SocketAddr,
    ) -> Result<Vec<u8>> {
        let mut token = vec![];
        // ip
        match src.ip() {
            std::net::IpAddr::V4(ipv4_addr) => token.extend_from_slice(&ipv4_addr.octets()),
            std::net::IpAddr::V6(ipv6_addr) => token.extend_from_slice(&ipv6_addr.octets()),
        };

        let timestamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // timestamp
        token.extend_from_slice(&timestamp.to_be_bytes());
        // odcid
        token.extend_from_slice(dcid);

        // sha256
        let mut hasher = Sha256::new();
        // seed
        hasher.update(&self.0);
        // ip + timestamp + odcid
        hasher.update(&token);
        // new_scid
        hasher.update(&new_scid);

        token.extend_from_slice(&hasher.finish());

        Ok(token)
    }

    fn validate_address<'a>(
        &self,
        scid: &ConnectionId<'_>,
        dcid: &ConnectionId<'_>,
        src: &SocketAddr,
        token: &'a [u8],
    ) -> Option<ConnectionId<'a>> {
        let addr = match src.ip() {
            std::net::IpAddr::V4(a) => a.octets().to_vec(),
            std::net::IpAddr::V6(a) => a.octets().to_vec(),
        };

        // token length is too short.
        if addr.len() + 40 > token.len() {
            return None;
        }

        // invalid address.
        if addr != &token[..addr.len()] {
            return None;
        }

        let timestamp = Duration::from_secs(u64::from_be_bytes(
            token[addr.len()..addr.len() + 8].try_into().unwrap(),
        ));
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap();

        // timeout
        if now - timestamp > self.1 {
            return None;
        }

        let sha256 = &token[token.len() - 32..];

        // sha256
        let mut hasher = Sha256::new();
        // seed
        hasher.update(&self.0);
        // ip + timestamp + odcid
        hasher.update(&token[..token.len() - 32]);
        // new_scid
        hasher.update(&dcid);

        // sha256 check error.
        if sha256 != hasher.finish() {
            return None;
        }

        Some(ConnectionId::from_ref(
            &token[addr.len() + 8..token.len() - 32],
        ))
    }
}

/// Quic connection acceptor for server-side.
pub struct Acceptor {
    /// configuration for quiche `Connection`.
    config: quiche::Config,
    /// Algorithem for address validation.
    address_validator: Box<dyn AddressValidator>,
}

impl Acceptor {
    /// Create a new acceptor with custom `quiche::Config` and `AddressValidator`
    pub fn new<A: AddressValidator + 'static>(
        config: quiche::Config,
        address_validator: A,
    ) -> Self {
        Self {
            config,
            address_validator: Box::new(address_validator),
        }
    }

    pub fn recv(
        &mut self,
        poll: &Poll,
        buf: &mut [u8],
        recv_size: usize,
        recv_info: RecvInfo,
    ) -> Result<Option<(usize, SendInfo)>> {
        let header = quiche::Header::from_slice(&mut buf[..recv_size], quiche::MAX_CONN_ID_LEN)?;

        match poll.recv_with(&header.dcid, buf, recv_size, recv_info) {
            Err(Error::NotFound) => {
                if let quiche::Type::Initial = header.ty {
                    self.initial(header, buf, recv_size, recv_info)
                } else {
                    log::warn!(
                        "recv unsupport packet, scid={:?}, dcid={:?}, from={}, to={}, ty={:?}",
                        header.scid,
                        header.dcid,
                        recv_info.from,
                        recv_info.to,
                        header.ty
                    );

                    Ok(None)
                }
            }
            r => r,
        }
    }

    fn initial(
        &mut self,
        header: Header<'_>,
        buf: &mut [u8],
        read_size: usize,
        recv_info: RecvInfo,
    ) -> Result<Option<(usize, SendInfo)>> {
        // send Version negotiation packet.
        if !quiche::version_is_supported(header.version) {
            return self.negotiate_version(header, buf, read_size, recv_info);
        }

        // Safety: present in `Initial` packet.
        let token = header.token.as_ref().unwrap();

        // send retry packet.
        if token.is_empty() {
            return self.retry(header, buf, read_size, recv_info);
        }

        let odcid = match self.address_validator.validate_address(
            &header.scid,
            &header.dcid,
            &recv_info.from,
            token,
        ) {
            Some(odcid) => odcid,
            None => {
                log::error!(
                    "failed to validate address, from={:?}, to={}, scid={:?}, dcid={:?}",
                    recv_info.from,
                    recv_info.to,
                    header.scid,
                    header.dcid
                );
                return Ok(None);
            }
        };

        let mut quiche_conn = match quiche::accept(
            &header.dcid,
            Some(&odcid),
            recv_info.to,
            recv_info.from,
            &mut self.config,
        ) {
            Ok(conn) => {
                log::trace!(
                    "QuicServer(initial) accept new conn, from={:?}, to={}, scid={:?}, dcid={:?}, odcid={:?}",
                    recv_info.from,
                    recv_info.to,
                    header.scid,
                    header.dcid,
                    odcid
                );
                conn
            }
            Err(err) => {
                log::error!(
                    "failed to accept connection, from={:?}, to={}, scid={:?}, dcid={:?}, err={}",
                    recv_info.from,
                    recv_info.to,
                    header.scid,
                    header.dcid,
                    err
                );
                return Ok(None);
            }
        };

        if let Err(err) = quiche_conn.recv(&mut buf[..read_size], recv_info) {
            log::error!(
                "failed to recv data, from={:?}, to={}, scid={:?}, dcid={:?}, err={}",
                recv_info.from,
                recv_info.to,
                header.scid,
                header.dcid,
                err
            );
            return Ok(None);
        }

        return Ok(None);
    }

    fn retry(
        &self,
        header: Header<'_>,
        buf: &mut [u8],
        _recv_size: usize,
        recv_info: RecvInfo,
    ) -> Result<Option<(usize, SendInfo)>> {
        let new_scid = random_conn_id();

        log::trace!(
            "retry, from={:?}, to={}, scid={:?}, dcid={:?}, new_scid={:?}",
            recv_info.from,
            recv_info.to,
            header.scid,
            header.dcid,
            new_scid
        );

        let token = self.address_validator.mint_retry_token(
            &header.scid,
            &header.dcid,
            &new_scid,
            &recv_info.from,
        )?;

        let send_size = match quiche::retry(
            &header.scid,
            &header.dcid,
            &new_scid,
            &token,
            header.version,
            buf,
        ) {
            Ok(send_size) => send_size,
            Err(err) => {
                log::error!(
                    "failed to generate retry packet, from={:?}, to={}, scid={:?}, dcid={:?}, err={}",
                    recv_info.from,
                    recv_info.to,
                    header.scid,
                    header.dcid,
                    err
                );
                return Ok(None);
            }
        };

        Ok(Some((
            send_size,
            SendInfo {
                from: recv_info.to,
                to: recv_info.from,
                at: Instant::now(),
            },
        )))
    }

    fn negotiate_version(
        &self,
        header: Header<'_>,
        buf: &mut [u8],
        _recv_size: usize,
        recv_info: RecvInfo,
    ) -> Result<Option<(usize, SendInfo)>> {
        log::trace!(
            "negotiate_version, from={:?}, to={}, scid={:?}, dcid={:?}",
            recv_info.from,
            recv_info.to,
            header.scid,
            header.dcid
        );

        let send_size = match quiche::negotiate_version(&header.scid, &header.dcid, buf) {
            Ok(send_size) => send_size,
            Err(err) => {
                log::error!(
                    "failed to generate negotiation_version packet, from={:?}, to={}, scid={:?}, dcid={:?}, err={}",
                    recv_info.from,
                    recv_info.to,
                    header.scid,
                    header.dcid,
                    err
                );
                return Ok(None);
            }
        };

        Ok(Some((
            send_size,
            SendInfo {
                from: recv_info.to,
                to: recv_info.from,
                at: Instant::now(),
            },
        )))
    }
}

#[cfg(test)]
mod tests {
    use std::{net::SocketAddr, thread::sleep, time::Duration};

    use crate::utils::random_conn_id;

    use super::*;

    #[test]
    fn test_default_address_validator() {
        let _validator = SimpleAddressValidator::new(Duration::from_secs(100));

        let scid = random_conn_id();
        let dcid = random_conn_id();
        let new_scid = random_conn_id();

        let src: SocketAddr = "127.0.0.1:1234".parse().unwrap();

        let token = _validator
            .mint_retry_token(&scid, &dcid, &new_scid, &src)
            .unwrap();

        assert_eq!(
            _validator.validate_address(&scid, &new_scid, &src, &token),
            Some(dcid.clone())
        );

        assert_eq!(
            _validator.validate_address(&scid, &dcid, &src, &token),
            None
        );

        assert_eq!(
            _validator.validate_address(&scid, &new_scid, &src, &token),
            Some(dcid.clone())
        );

        let src: SocketAddr = "0.0.0.0:1234".parse().unwrap();

        assert_eq!(
            _validator.validate_address(&scid, &new_scid, &src, &token),
            None
        );

        let _validator = SimpleAddressValidator::new(Duration::from_secs(1));

        let token = _validator
            .mint_retry_token(&scid, &dcid, &new_scid, &src)
            .unwrap();

        assert_eq!(
            _validator.validate_address(&scid, &new_scid, &src, &token),
            Some(dcid.clone())
        );

        sleep(Duration::from_secs(2));

        assert_eq!(
            _validator.validate_address(&scid, &new_scid, &src, &token),
            None
        );

        // timeout.
    }
}
