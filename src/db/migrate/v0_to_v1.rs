//! Encode most pano IDs as u32s.

use std::{fs, hash::Hash, io::Cursor, sync::Arc};

use byteorder::{LE, ReadBytesExt, WriteBytesExt};
use compact_str::CompactString;
use heed::{
    Database, Env, EnvOpenOptions, RwTxn,
    types::{Bytes, Str, U32},
};
use tracing::info;

use crate::{
    model::{ApiPanoId, PanoId, SizedTile},
    streetview::api::is_third_party_pano,
};

const OLD_VERSION: u32 = 0;
const NEW_VERSION: u32 = 1;

struct OldDb {
    env: Env,
    getmetadata_db: Database<Str, Bytes>,
    listentityphotos_db: Database<SizedTile, Bytes>,
    settings_db: Database<Str, Bytes>,
}
pub struct NewDb {
    env: Env,
    getmetadata_db: Database<U32<LE>, Bytes>,
    listentityphotos_db: Database<SizedTile, Bytes>,
    /// Mapping of Streetview pano IDs into our internal u32 representation.
    pano_ids_db: Database<Str, U32<LE>>,
    settings_db: Database<Str, Bytes>,
}

pub fn migrate() -> eyre::Result<()> {
    fs::rename("cache", format!("cache-v{OLD_VERSION}")).unwrap();
    fs::create_dir("cache").unwrap();

    let old_db = OldDb::new()?;
    let new_db = NewDb::new()?;

    let old_txn = old_db.env.read_txn()?;
    let mut new_txn = new_db.env.write_txn()?;

    // migrate getmetadata
    for entry in old_db.getmetadata_db.iter(&old_txn)? {
        let (old_key, old_data) = entry?;
        let new_key = new_db.get_pano_id(&mut new_txn, old_key);

        let old_getmetadata = decode_v0_getmetadata(Cursor::new(old_data));
        let mut new_getmetadata = Vec::new();
        for old_link in old_getmetadata {
            let new_link_pano_id = new_db.get_pano_id(&mut new_txn, &old_link.pano.id.0);
            new_getmetadata.push(V1PanoLink {
                pano: V1Pano {
                    id: new_link_pano_id,
                    loc: old_link.pano.loc,
                },
                heading: old_link.heading,
            });
        }

        info!(
            "getmetadata {old_key:?} to {new_key:?} - {} links",
            new_getmetadata.len()
        );

        let new_data = encode_v1_getmetadata(&new_getmetadata);
        new_db
            .getmetadata_db
            .put(&mut new_txn, &new_key.0, &new_data)
            .unwrap();
    }

    // migrate listentityphotos
    for entry in old_db.listentityphotos_db.iter(&old_txn)? {
        let (key, old_data) = entry?;
        info!("listentityphotos {key:?}");

        let old_listentityphotos = decode_v0_listentityphotos(Cursor::new(old_data));
        let new_listentityphotos = if let Some(old_listentityphotos) = old_listentityphotos {
            let mut new_listentityphotos = Vec::new();
            for old_pano in old_listentityphotos.iter() {
                let new_pano_id = new_db.get_pano_id(&mut new_txn, &old_pano.id.0);
                new_listentityphotos.push(V1Pano {
                    id: new_pano_id,
                    loc: old_pano.loc,
                });
            }
            Some(new_listentityphotos.into())
        } else {
            None
        };

        let new_data = encode_v1_listentityphotos(new_listentityphotos);
        new_db
            .listentityphotos_db
            .put(&mut new_txn, &key, &new_data)
            .unwrap();
    }

    // migrate settings
    for entry in old_db.settings_db.iter(&old_txn)? {
        let (key, old_data) = entry?;
        new_db.settings_db.put(&mut new_txn, key, old_data).unwrap();
    }
    new_db
        .settings_db
        .put(
            &mut new_txn,
            "version",
            NEW_VERSION.to_le_bytes().as_slice(),
        )
        .unwrap();

    old_txn.commit()?;
    new_txn.commit()?;

    old_db.env.prepare_for_closing().wait();
    new_db.env.prepare_for_closing().wait();

    Ok(())
}

impl OldDb {
    fn new() -> eyre::Result<OldDb> {
        let env = unsafe {
            EnvOpenOptions::new()
                .max_dbs(3)
                .map_size(1024 * 1024 * 1024 * 128)
                .open(format!("./cache-v{OLD_VERSION}"))?
        };
        let mut wtxn = env.write_txn()?;
        let getmetadata_db = env.create_database(&mut wtxn, Some("getmetadata"))?;
        let listentityphotos_db = env.create_database(&mut wtxn, Some("listentityphotos"))?;
        let settings_db = env.create_database(&mut wtxn, Some("settings"))?;
        wtxn.commit()?;

        Ok(OldDb {
            env,
            getmetadata_db,
            listentityphotos_db,
            settings_db,
        })
    }
}
impl NewDb {
    fn new() -> eyre::Result<NewDb> {
        let env = unsafe {
            EnvOpenOptions::new()
                .max_dbs(4)
                .map_size(1024 * 1024 * 1024 * 128)
                .open("./cache")?
        };
        let mut wtxn = env.write_txn()?;

        let settings_db = env.create_database(&mut wtxn, Some("settings"))?;
        let getmetadata_db = env.create_database(&mut wtxn, Some("getmetadata"))?;
        let listentityphotos_db = env.create_database(&mut wtxn, Some("listentityphotos"))?;
        let pano_ids_db = env.create_database(&mut wtxn, Some("panoids"))?;

        wtxn.commit()?;

        Ok(NewDb {
            env,
            getmetadata_db,
            listentityphotos_db,
            pano_ids_db,
            settings_db,
        })
    }

    pub fn get_pano_id(&self, txn: &mut RwTxn<'_>, str_pano_id: &str) -> PanoId {
        if let Some(pano_id) = self.pano_ids_db.get(txn, str_pano_id).unwrap() {
            return PanoId(pano_id);
        };

        let mut expected_pano_id = self.next_pano_id(txn);
        assert!(expected_pano_id < 2u32.pow(31), "pano id overflow");
        if is_third_party_pano(str_pano_id) {
            expected_pano_id |= 1 << 31;
        }

        self.pano_ids_db
            .put(txn, str_pano_id, &expected_pano_id)
            .unwrap();

        PanoId(expected_pano_id)
    }
    fn next_pano_id(&self, txn: &mut RwTxn<'_>) -> u32 {
        let next_pano_id = self
            .settings_db
            .get(txn, "next-pano-id")
            .unwrap()
            .unwrap_or_default();
        let next_pano_id = if next_pano_id.is_empty() {
            0
        } else {
            u32::from_le_bytes(
                next_pano_id
                    .try_into()
                    .expect("next-pano-id should be a valid u32"),
            )
        };
        self.settings_db
            .put(
                txn,
                "next-pano-id",
                &(next_pano_id
                    .checked_add(1)
                    .expect("pano id overflow, maybe the internal pano id representation needs to be replaced with a u64?"))
                .to_le_bytes(),
            )
            .unwrap();
        next_pano_id
    }
}

fn encode_v1_getmetadata(links: &[V1PanoLink]) -> Vec<u8> {
    let mut buf = Vec::new();

    buf.write_u8(u8::try_from(links.len()).unwrap()).unwrap();
    for link in links {
        write_v1_pano_id(&mut buf, &link.pano.id);
        buf.write_f32::<LE>(link.heading).unwrap();
        buf.write_f64::<LE>(link.pano.loc.lat).unwrap();
        buf.write_f64::<LE>(link.pano.loc.lng).unwrap();
    }

    buf
}
fn decode_v0_getmetadata(mut cur: Cursor<&[u8]>) -> Box<[V0PanoLink]> {
    let mut links = Vec::new();

    let link_count = cur.read_u8().unwrap();
    for _ in 0..link_count {
        let id = read_pano_id(&mut cur);
        let heading = cur.read_f32::<LE>().unwrap();
        let lat = cur.read_f64::<LE>().unwrap();
        let lng = cur.read_f64::<LE>().unwrap();
        links.push(V0PanoLink {
            pano: V0Pano {
                id,
                loc: Location { lat, lng },
            },
            heading,
        });
    }

    links.into()
}

fn encode_v1_listentityphotos(panos: Option<Arc<[V1Pano]>>) -> Vec<u8> {
    let mut buf = Vec::new();

    if let Some(panos) = panos {
        // 1 = normal
        buf.write_u8(1).unwrap();
        for pano in panos.iter() {
            write_v1_pano_id(&mut buf, &pano.id);
            buf.write_f64::<LE>(pano.loc.lat).unwrap();
            buf.write_f64::<LE>(pano.loc.lng).unwrap();
        }
    } else {
        // 0 = too big, smaller pano should be checked
        buf.write_u8(0).unwrap();
    }

    buf
}
fn decode_v0_listentityphotos(mut cur: Cursor<&[u8]>) -> Option<Arc<[V0Pano]>> {
    let mut panos = Vec::new();

    let header = cur.read_u8().unwrap();
    if header == 0 {
        return None;
    }

    while cur.position() < cur.get_ref().len() as u64 {
        let id = read_pano_id(&mut cur);

        let lat = cur.read_f64::<LE>().unwrap();
        let lng = cur.read_f64::<LE>().unwrap();
        let loc = Location { lat, lng };
        let pano = V0Pano { id, loc };
        panos.push(pano);
    }

    Some(panos.into())
}

fn write_v1_pano_id(buf: &mut Vec<u8>, pano_id: &PanoId) {
    buf.write_u32::<LE>(pano_id.0).unwrap();
}

fn read_pano_id(cur: &mut Cursor<&[u8]>) -> ApiPanoId {
    let id_len = cur.read_u8().unwrap();
    // let mut id_bytes = vec![0; id_len as usize];
    // cur.read_exact(&mut id_bytes).unwrap();
    let cur_pos = cur.position() as usize;
    let end_pos = cur_pos + id_len as usize;
    let id_bytes = &cur.get_ref()[cur_pos..end_pos];
    cur.set_position(end_pos as u64);

    let s = CompactString::from_utf8(id_bytes).unwrap();

    ApiPanoId(s)
}

#[derive(Debug, Clone, PartialEq, Hash)]
struct V0Pano {
    id: ApiPanoId,
    loc: Location,
}
#[derive(Debug, Clone)]
struct V0PanoLink {
    /// For GetMetadata links, the location will be a game loc.
    pub pano: V0Pano,
    pub heading: f32,
}

#[derive(Debug, Clone, PartialEq, Hash)]
struct V1Pano {
    pub id: PanoId,
    loc: Location,
}
#[derive(Debug, Clone)]
struct V1PanoLink {
    /// For GetMetadata links, the location will be a game loc.
    pub pano: V1Pano,
    pub heading: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Location {
    /// y
    pub lat: f64,
    /// x
    pub lng: f64,
}
impl Hash for Location {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.lat.to_bits().hash(state);
        self.lng.to_bits().hash(state);
    }
}
