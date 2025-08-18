use quiche::{Connection, Header, RecvInfo};

use crate::{Error, Group, Result, Token, utils::random_conn_id, validation::AddressValidator};

/// Handshake result, returns by [`handshake`](Acceptor::handshake) function.
pub enum Handshake {
    Handshake(usize),
    Accept(Connection),
}

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

    /// Process quic handshake
    pub fn handshake(
        &mut self,
        header: Header<'_>,
        buf: &mut [u8],
        read_size: usize,
        recv_info: RecvInfo,
    ) -> Result<Handshake> {
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
                return Err(Error::ValidateAddress);
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
                return Err(Error::Quiche(err));
            }
        };

        Ok(Handshake::Accept(quiche_conn))
    }

    fn retry(
        &self,
        header: Header<'_>,
        buf: &mut [u8],
        _recv_size: usize,
        recv_info: RecvInfo,
    ) -> Result<Handshake> {
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
                return Err(Error::Quiche(err));
            }
        };

        Ok(Handshake::Handshake(send_size))
    }

    fn negotiate_version(
        &self,
        header: Header<'_>,
        buf: &mut [u8],
        _recv_size: usize,
        recv_info: RecvInfo,
    ) -> Result<Handshake> {
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
                return Err(Error::Quiche(err));
            }
        };

        Ok(Handshake::Handshake(send_size))
    }
}

/// Handshake result, returns by [`handshake`](Acceptor::handshake) function.
pub enum Accept {
    Handshake(usize),
    Accept(Token),
    Recv(usize),
}

impl Group {
    /// The server socket uses this function instead of `recv` to handle accepting data.
    pub fn accept_recv(
        &self,
        acceptor: &mut Acceptor,
        buf: &mut [u8],
        recv_size: usize,
        recv_info: RecvInfo,
    ) -> Result<Accept> {
        let header = quiche::Header::from_slice(&mut buf[..recv_size], quiche::MAX_CONN_ID_LEN)
            .map_err(Error::Quiche)?;

        match self.recv_with_(&header.scid, &mut buf[..recv_size], recv_info) {
            Ok(read_size) => Ok(Accept::Recv(read_size)),
            Err(Error::NotFound) => match acceptor.handshake(header, buf, recv_size, recv_info) {
                Ok(Handshake::Accept(conn)) => Ok(Accept::Accept(self.register(conn)?)),
                Ok(Handshake::Handshake(send_size)) => Ok(Accept::Handshake(send_size)),
                Err(err) => Err(err),
            },
            Err(err) => Err(err),
        }
    }
}
