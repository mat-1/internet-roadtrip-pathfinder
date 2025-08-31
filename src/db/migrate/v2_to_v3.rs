//! Convert locations from two f64s to two i32s.

use std::{fs, io::Cursor, sync::Arc};

use byteorder::{LE, ReadBytesExt, WriteBytesExt};
use heed::{
    Database, Env, EnvOpenOptions,
    types::{Bytes, Str, U32},
};
use tracing::info;

use crate::model::{PanoId, SizedTile};

const OLD_VERSION: u32 = 2;
const NEW_VERSION: u32 = 3;

struct OldDb {
    env: Env,
    getmetadata_db: Database<U32<LE>, Bytes>,
    listentityphotos_db: Database<SizedTile, Bytes>,
    /// Mapping of Streetview pano IDs into our internal u32 representation.
    pano_ids_db: Database<Str, U32<LE>>,
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

    info!("Migrating getmetadata_db");
    // migrate getmetadata
    for entry in old_db.getmetadata_db.iter(&old_txn)? {
        let (key, old_data) = entry?;

        let (old_loc, old_links) = decode_old_getmetadata(&mut Cursor::new(old_data));
        let new_loc = translate_location(old_loc);
        let mut new_getmetadata = Vec::new();
        for old_link in old_links {
            new_getmetadata.push(NewPanoLink {
                pano: NewPano {
                    id: old_link.pano.id,
                    loc: translate_location(old_link.pano.loc),
                },
                heading: old_link.heading,
            });
        }

        let new_data = encode_new_getmetadata(new_loc, &new_getmetadata);
        new_db
            .getmetadata_db
            .put(&mut new_txn, &key, &new_data)
            .unwrap();
    }

    // migrate listentityphotos
    for entry in old_db.listentityphotos_db.iter(&old_txn)? {
        let (key, old_data) = entry?;
        info!("listentityphotos {key:?}");

        let old_listentityphotos = decode_old_listentityphotos(&mut Cursor::new(old_data));
        let new_listentityphotos = if let Some(old_listentityphotos) = old_listentityphotos {
            let mut new_listentityphotos = Vec::new();
            for old_pano in old_listentityphotos.iter() {
                new_listentityphotos.push(NewPano {
                    id: old_pano.id,
                    loc: translate_location(old_pano.loc),
                });
            }
            Some(new_listentityphotos.into())
        } else {
            None
        };

        let new_data = encode_new_listentityphotos(new_listentityphotos);
        new_db
            .listentityphotos_db
            .put(&mut new_txn, &key, &new_data)
            .unwrap();
    }

    // migrate pano ids
    for entry in old_db.pano_ids_db.iter(&old_txn)? {
        let (key, old_data) = entry?;
        new_db
            .pano_ids_db
            .put(&mut new_txn, key, &old_data)
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
                .max_dbs(4)
                .map_size(1024 * 1024 * 1024 * 128)
                .open(format!("./cache-v{OLD_VERSION}"))?
        };
        let mut wtxn = env.write_txn()?;
        let settings_db = env.create_database(&mut wtxn, Some("settings"))?;
        let getmetadata_db = env.create_database(&mut wtxn, Some("getmetadata"))?;
        let listentityphotos_db = env.create_database(&mut wtxn, Some("listentityphotos"))?;
        let pano_ids_db = env.create_database(&mut wtxn, Some("panoids"))?;
        wtxn.commit()?;

        Ok(OldDb {
            env,
            getmetadata_db,
            listentityphotos_db,
            pano_ids_db,
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
}

fn translate_location(old_loc: OldLocation) -> NewLocation {
    NewLocation {
        lat_i: deg_to_i(old_loc.lat),
        lng_i: deg_to_i(old_loc.lng),
    }
}

fn decode_old_getmetadata(cur: &mut Cursor<&[u8]>) -> (OldLocation, Box<[OldPanoLink]>) {
    let this_loc = old_read_location(cur);

    let mut links = Vec::new();
    let mut link_count = cur.read_u8().unwrap() as u32;
    if link_count == 255 {
        link_count = cur.read_u32::<LE>().unwrap();
    }

    for _ in 0..link_count {
        let id = read_pano_id(cur);
        let heading = cur.read_f32::<LE>().unwrap();
        let loc = old_read_location(cur);
        links.push(OldPanoLink {
            pano: OldPano { id, loc },
            heading,
        });
    }

    (this_loc, links.into())
}
fn encode_new_getmetadata(loc: NewLocation, links: &[NewPanoLink]) -> Vec<u8> {
    let mut buf = Vec::new();

    new_write_location(&mut buf, loc);

    let num_links = links.len();
    if links.len() >= 255 {
        buf.write_u8(255).unwrap();
        buf.write_u32::<LE>(num_links.try_into().unwrap()).unwrap();
    } else {
        buf.write_u8(num_links as u8).unwrap();
    }

    for link in links {
        write_pano_id(&mut buf, &link.pano.id);
        buf.write_f32::<LE>(link.heading).unwrap();
        new_write_location(&mut buf, link.pano.loc);
    }

    buf
}

fn old_read_location(cur: &mut Cursor<&[u8]>) -> OldLocation {
    let lat = cur.read_f64::<LE>().unwrap();
    let lng = cur.read_f64::<LE>().unwrap();
    OldLocation { lat, lng }
}
fn new_write_location(buf: &mut Vec<u8>, loc: NewLocation) {
    buf.write_i32::<LE>(loc.lat_i).unwrap();
    buf.write_i32::<LE>(loc.lng_i).unwrap();
}

fn write_pano_id(buf: &mut Vec<u8>, pano_id: &PanoId) {
    buf.write_u32::<LE>(pano_id.0).unwrap();
}
fn read_pano_id(cur: &mut Cursor<&[u8]>) -> PanoId {
    PanoId(cur.read_u32::<LE>().unwrap())
}

fn encode_new_listentityphotos(panos: Option<Arc<[NewPano]>>) -> Vec<u8> {
    let mut buf = Vec::new();

    if let Some(panos) = panos {
        // 1 = normal
        buf.write_u8(1).unwrap();
        for pano in panos.iter() {
            write_pano_id(&mut buf, &pano.id);
            new_write_location(&mut buf, pano.loc);
        }
    } else {
        // 0 = too big, smaller pano should be checked
        buf.write_u8(0).unwrap();
    }

    buf
}
fn decode_old_listentityphotos(cur: &mut Cursor<&[u8]>) -> Option<Box<[OldPano]>> {
    let mut panos = Vec::new();

    let header = cur.read_u8().unwrap();
    if header == 0 {
        return None;
    }

    while cur.position() < cur.get_ref().len() as u64 {
        let id = read_pano_id(cur);
        let loc = old_read_location(cur);
        let pano = OldPano { id, loc };
        panos.push(pano);
    }

    Some(panos.into())
}

#[derive(Clone, Copy)]
struct OldLocation {
    lat: f64,
    lng: f64,
}
struct OldPanoLink {
    pano: OldPano,
    heading: f32,
}
struct OldPano {
    id: PanoId,
    loc: OldLocation,
}

#[derive(Clone, Copy)]
struct NewLocation {
    lat_i: i32,
    lng_i: i32,
}
struct NewPanoLink {
    pano: NewPano,
    heading: f32,
}
struct NewPano {
    id: PanoId,
    loc: NewLocation,
}

fn deg_to_i(deg: f64) -> i32 {
    (deg / 180. * i32::MAX as f64) as i32
}
