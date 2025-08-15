use std::time::Instant;

use quiche::{Header, RecvInfo, SendInfo};

use crate::{AddressValidator, Error, Group, Result, utils::random_conn_id};

/// Accept new inbound `quic` connection.
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

    /// The server side should use this function instead of directly calling the [`Group::recv_and_send`] function.
    pub fn recv_and_send(
        &mut self,
        poll: &Group,
        buf: &mut [u8],
        recv_size: usize,
        recv_info: RecvInfo,
    ) -> Result<Option<(usize, SendInfo)>> {
        let header = quiche::Header::from_slice(&mut buf[..recv_size], quiche::MAX_CONN_ID_LEN)?;

        match poll.recv_and_send_with(&header.dcid, buf, recv_size, recv_info) {
            Err(Error::NotFound) => {
                if let quiche::Type::Initial = header.ty {
                    self.initial(poll, header, buf, recv_size, recv_info)
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
        poll: &Group,
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

        let quiche_conn = match quiche::accept(
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

        // Register to poll.
        let _ = poll.register(quiche_conn)?;

        // Process recv packet from peer.
        match poll.recv_and_send_with(&header.dcid, buf, read_size, recv_info) {
            Err(Error::Busy) | Err(Error::NotFound) | Err(Error::Retry) => unreachable!(""),
            r => r,
        }
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
