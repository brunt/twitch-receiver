use anyhow::Context;
use iroh::{discovery::dns::DnsDiscovery, Endpoint, RelayMode, SecretKey};
use iroh_blobs::{
    format::collection::Collection,
    get::{
        fsm::{AtBlobHeaderNextError, DecodeError},
        request::get_hash_seq_and_sizes,
    },
    store::ExportMode,
    ticket::BlobTicket,
    HashAndFormat,
};
use std::{
    path::{Path, PathBuf},
    str::FromStr,
};

/// Get the secret key or generate a new one.
///
/// Print the secret key to stderr if it was generated, so the user can save it.
fn get_or_create_secret(print: bool) -> anyhow::Result<SecretKey> {
    match std::env::var("IROH_SECRET") {
        Ok(secret) => SecretKey::from_str(&secret).context("invalid secret"),
        Err(_) => {
            let key = SecretKey::generate(rand::rngs::OsRng);
            if print {
                eprintln!("using secret key {}", key);
            }
            Ok(key)
        }
    }
}

fn validate_path_component(component: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        !component.contains('/'),
        "path components must not contain the only correct path separator, /"
    );
    Ok(())
}

fn get_export_path(root: &Path, name: &str) -> anyhow::Result<PathBuf> {
    let parts = name.split('/');
    let mut path = root.to_path_buf();
    for part in parts {
        validate_path_component(part)?;
        path.push(part);
    }
    Ok(path)
}

async fn export(
    db: impl iroh_blobs::store::Store,
    collection: Collection,
    root: &Path,
) -> anyhow::Result<()> {
    for (name, hash) in collection.iter() {
        let target = get_export_path(root, name)?;
        if target.exists() {
            eprintln!(
                "target {} already exists. Export stopped.",
                target.display()
            );
            eprintln!("You can remove the file or directory and try again. The download will not be repeated.");
            anyhow::bail!("target {} already exists", target.display());
        }
        db.export(
            *hash,
            target,
            ExportMode::Copy,
            Box::new(move |_position| Ok(())),
        )
        .await?;
    }
    Ok(())
}

fn show_get_error(e: anyhow::Error) -> anyhow::Error {
    if let Some(err) = e.downcast_ref::<DecodeError>() {
        match err {
            DecodeError::NotFound => {
                eprintln!("send side no longer has a file")
            }
            DecodeError::LeafNotFound(_) | DecodeError::ParentNotFound(_) => {
                eprintln!("send side no longer has part of a file")
            }
            DecodeError::Io(err) => eprintln!("generic network error: {}", err),
            DecodeError::Read(err) => {
                eprintln!("error reading data from quinn: {}", err)
            }
            DecodeError::LeafHashMismatch(_) | DecodeError::ParentHashMismatch(_) => {
                eprintln!("send side sent wrong data")
            }
        };
    } else if let Some(header_error) = e.downcast_ref::<AtBlobHeaderNextError>() {
        // TODO(iroh-bytes): get_to_db should have a concrete error type so you don't have to guess
        match header_error {
            AtBlobHeaderNextError::Io(err) => {
                eprintln!("generic network error: {}", err)
            }
            AtBlobHeaderNextError::Read(err) => {
                eprintln!("error reading data from quinn: {}", err)
            }
            AtBlobHeaderNextError::NotFound => {
                eprintln!("send side no longer has a file")
            }
        };
    } else {
        eprintln!("generic error: {:?}", e.root_cause());
    }
    e
}

pub async fn receive(ticket: BlobTicket, dest: &Path) -> anyhow::Result<()> {
    let addr = ticket.node_addr().clone();
    let secret_key = get_or_create_secret(false)?;
    let mut builder = Endpoint::builder()
        .alpns(vec![])
        .secret_key(secret_key)
        .relay_mode(RelayMode::Default);

    if ticket.node_addr().relay_url.is_none() && ticket.node_addr().direct_addresses.is_empty() {
        builder = builder.add_discovery(|_| Some(DnsDiscovery::n0_dns()));
    }
    let endpoint = builder.bind().await?;
    let db = iroh_blobs::store::mem::Store::new();
    if let Ok(connection) = endpoint.connect(addr, iroh_blobs::protocol::ALPN).await {
        let hash_and_format = HashAndFormat {
            hash: ticket.hash(),
            format: ticket.format(),
        };
        let progress = iroh_blobs::util::progress::IgnoreProgressSender::default();
        let (_hash_seq, _) =
            get_hash_seq_and_sizes(&connection, &hash_and_format.hash, 1024 * 1024 * 32)
                .await
                .map_err(show_get_error)?;
        let get_conn = || async move { Ok(connection) };
        let _stats = iroh_blobs::get::db::get_to_db(&db, get_conn, &hash_and_format, progress)
            .await
            .map_err(|e| show_get_error(anyhow::anyhow!(e)))?;
        let collection = Collection::load_db(&db, &hash_and_format.hash).await?;
        if let Some((name, _)) = collection.iter().next() {
            if let Some(first) = name.split('/').next() {
                println!("downloading to: {};", first);
            }
        }
        export(db, collection, dest).await?;
    }

    Ok(())
}
