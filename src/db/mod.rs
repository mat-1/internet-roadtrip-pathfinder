pub mod migrate;

use std::{
    borrow::Cow,
    fs,
    io::Cursor,
    path::PathBuf,
    sync::{Arc, LazyLock},
};

use byteorder::{BE, LE, ReadBytesExt, WriteBytesExt};
use heed::{
    BoxedError, BytesDecode, BytesEncode, Database, Env, EnvOpenOptions, RoTxn, RwTxn, WithTls,
    types::*,
};
use tracing::info;

use crate::{
    db::migrate::CURRENT_VERSION,
    math::angle::Angle,
    model::{
        GetMetadataResponse, Location, Pano, PanoId, PanoLink, PanoWithBothLocations, SizedTile,
        SmallTile,
    },
    streetview::api::{decode_protobuf_pano, is_third_party_pano},
};

pub static DB: LazyLock<Db> = LazyLock::new(|| Db::new().unwrap());

pub struct Db {
    env: Env,
    getmetadata_db: Database<U32<BE>, Bytes>,
    listentityphotos_db: Database<SizedTile, Bytes>,
    /// Mapping of Streetview pano IDs into our internal u32 representation.
    pub pano_ids_db: Database<Str, U32<LE>>,
    settings_db: Database<Str, Bytes>,
}
impl Db {
    pub fn new() -> eyre::Result<Self> {
        info!("Initializing database");

        let mut first_run = false;

        let path = PathBuf::from("./cache");
        if !path.exists() {
            fs::create_dir(&path).unwrap();
            first_run = true;
        }
        // SAFETY: The file shouldn't be modified by anything other than heed.
        let env = unsafe {
            EnvOpenOptions::new()
                .max_dbs(4)
                .map_size(1024 * 1024 * 1024 * 128)
                .open(path)?
        };

        let mut wtxn = env.write_txn()?;
        let settings_db = env.create_database(&mut wtxn, Some("settings"))?;

        if first_run {
            settings_db
                .put(
                    &mut wtxn,
                    "version",
                    CURRENT_VERSION.to_le_bytes().as_slice(),
                )
                .unwrap();
        } else {
            let version = if let Some(data) = settings_db.get(&wtxn, "version").unwrap() {
                let data: &[u8] = data;
                u32::from_le_bytes(data.try_into().unwrap())
            } else {
                0
            };

            if version != CURRENT_VERSION {
                wtxn.abort();
                env.prepare_for_closing().wait();
                migrate::try_migrate_from_version(version);
                // try again
                return Db::new();
            }
        }

        let getmetadata_db = env.create_database(&mut wtxn, Some("getmetadata"))?;
        let listentityphotos_db = env.create_database(&mut wtxn, Some("listentityphotos"))?;
        let pano_ids_db = env.create_database(&mut wtxn, Some("panoids"))?;

        wtxn.commit().unwrap();

        info!("Finished initializing database");

        Ok(Self {
            env,
            getmetadata_db,
            listentityphotos_db,
            settings_db,
            pano_ids_db,
        })
    }

    /// Use the cache to convert a pano ID to its "game" coords and Streetview
    /// links, according to the GetMetadata API.
    pub fn lookup_getmetadata(&self, pano_id: &PanoId) -> Option<(Location, Box<[PanoLink]>)> {
        let txn = self.read_txn();
        let res = self.lookup_getmetadata_with_txn(&txn, pano_id);
        txn.commit().unwrap();
        res
    }
    pub fn lookup_getmetadata_with_txn(
        &self,
        txn: &RoTxn<'_>,
        pano_id: &PanoId,
    ) -> Option<(Location, Box<[PanoLink]>)> {
        let data = self.getmetadata_db.get(txn, &pano_id.0).unwrap()?;
        Some(decode_getmetadata(&mut Cursor::new(data)))
    }

    /// A faster alternative to [`Self::lookup_getmetadata`] that won't
    /// try parsing the links.
    pub fn lookup_getmetadata_location(&self, pano_id: &PanoId) -> Option<Location> {
        let txn = self.read_txn();
        let res = self.lookup_getmetadata_location_with_txn(&txn, pano_id);
        txn.commit().unwrap();
        res
    }
    /// A faster alternative to [`Self::lookup_getmetadata_with_txn`] that won't
    /// try parsing the links.
    pub fn lookup_getmetadata_location_with_txn(
        &self,
        txn: &RoTxn<'_>,
        pano_id: &PanoId,
    ) -> Option<Location> {
        let data = self.getmetadata_db.get(txn, &pano_id.0).unwrap()?;
        Some(read_location(&mut Cursor::new(data)))
    }

    pub fn save_getmetadata(&self, res: &GetMetadataResponse) -> eyre::Result<()> {
        let mut txn = self.write_txn();
        let res = self.save_getmetadata_with_txn(&mut txn, res);
        txn.commit().unwrap();
        res
    }
    pub fn save_getmetadata_with_txn(
        &self,
        txn: &mut RwTxn<'_>,
        res: &GetMetadataResponse,
    ) -> eyre::Result<()> {
        self.getmetadata_db
            .put(txn, &res.id.0, &encode_getmetadata(res))?;
        Ok(())
    }

    pub fn lookup_listentityphotos(
        &self,
        tile: &SizedTile,
    ) -> Option<Option<Arc<[PanoWithBothLocations]>>> {
        let txn = self.read_txn();
        let res = self.lookup_listentityphotos_with_txn(&txn, tile);
        txn.commit().unwrap();

        res
    }
    pub fn lookup_listentityphotos_with_txn(
        &self,
        txn: &RoTxn<'_>,
        tile: &SizedTile,
    ) -> Option<Option<Arc<[PanoWithBothLocations]>>> {
        let data = self.listentityphotos_db.get(txn, tile).unwrap()?;

        Some(decode_listentityphotos(&mut Cursor::new(data)))
    }
    /// Returns true if the tile is fully cached (i.e. had less than 3000
    /// items).
    pub fn is_sized_tile_cached(&self, txn: &RoTxn<'_>, tile: &SizedTile) -> bool {
        if let Some(res) = self.listentityphotos_db.get(txn, tile).unwrap() {
            res[0] == 1
        } else {
            false
        }
    }
    pub fn is_tile_cached(&self, txn: &RoTxn<'_>, tile: &SmallTile) -> bool {
        for tile_size in tile.get_all_sizes() {
            if DB.is_sized_tile_cached(txn, &tile_size) {
                return true;
            }
        }
        false
    }

    pub fn save_listentityphotos(
        &self,
        tile: &SizedTile,
        panos: Option<Arc<[PanoWithBothLocations]>>,
    ) -> eyre::Result<()> {
        let mut txn = self.write_txn();
        self.save_listentityphotos_with_txn(&mut txn, tile, panos)?;
        txn.commit()?;
        Ok(())
    }
    pub fn save_listentityphotos_with_txn(
        &self,
        txn: &mut RwTxn<'_>,
        tile: &SizedTile,
        panos: Option<Arc<[PanoWithBothLocations]>>,
    ) -> eyre::Result<()> {
        self.listentityphotos_db
            .put(txn, tile, &encode_listentityphotos(panos))?;

        Ok(())
    }

    pub fn delete_listentityphotos(&self, tile: SizedTile) -> eyre::Result<()> {
        let mut txn = self.write_txn();
        self.listentityphotos_db.delete(&mut txn, &tile)?;
        txn.commit()?;

        Ok(())
    }

    pub fn slow_list_tiles(&self) -> Box<[SizedTile]> {
        let mut tiles = Vec::new();

        let txn = self.read_txn();
        for res in self.listentityphotos_db.iter(&txn).unwrap() {
            let (tile, _) = res.unwrap();
            tiles.push(tile);
        }

        tiles.into()
    }

    pub fn get_pano_id(&self, str_pano_id: &str) -> PanoId {
        let mut txn = self.write_txn();
        let pano_id = self.get_pano_id_with_txn(&mut txn, str_pano_id);
        txn.commit().unwrap();
        pano_id
    }
    pub fn get_pano_id_with_txn(&self, txn: &mut RwTxn<'_>, str_pano_id: &str) -> PanoId {
        // try to decode it, just in case
        let str_pano_id = decode_protobuf_pano(str_pano_id);

        if let Some(pano_id) = self.pano_ids_db.get(txn, &str_pano_id).unwrap() {
            return PanoId(pano_id);
        };

        let mut expected_pano_id = self.next_pano_id(txn);
        assert!(expected_pano_id < 2u32.pow(31), "pano id overflow");
        if is_third_party_pano(&str_pano_id) {
            // set the top bit to a 1 so if it's a photosphere so we can cheaply check it
            // later
            expected_pano_id |= 1 << 31;
        }

        self.pano_ids_db
            .put(txn, &str_pano_id, &expected_pano_id)
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

    pub fn get_pano_count(&self) -> u32 {
        let txn = self.read_txn();
        let next_pano_id = self
            .settings_db
            .get(&txn, "next-pano-id")
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
        txn.commit().unwrap();

        next_pano_id
    }

    pub fn read_txn(&self) -> RoTxn<'_, WithTls> {
        self.env.read_txn().expect("failed to get read txn")
    }
    pub fn write_txn(&self) -> RwTxn<'_> {
        self.env.write_txn().expect("failed to get write txn")
    }
}

pub fn encode_getmetadata(res: &GetMetadataResponse) -> Vec<u8> {
    let mut buf = Vec::new();

    write_location(&mut buf, res.loc);

    let num_links = res.links.len();
    if res.links.len() >= 255 {
        // rarely, we encounter panos with more than 255 links. here's an example:
        // CAoSF0NJSE0wb2dLRUlDQWdJRGE4X3lMd2dF
        buf.write_u8(255).unwrap();
        buf.write_u32::<LE>(num_links.try_into().unwrap()).unwrap();
    } else {
        buf.write_u8(num_links as u8).unwrap();
    }

    for link in &res.links {
        write_pano_id(&mut buf, &link.pano.id);
        buf.write_f32::<LE>(link.heading).unwrap();
        write_location(&mut buf, link.pano.loc);
    }

    buf
}
pub fn decode_getmetadata(cur: &mut Cursor<&[u8]>) -> (Location, Box<[PanoLink]>) {
    let this_loc = read_location(cur);

    let mut links = Vec::new();
    let mut link_count = cur.read_u8().unwrap() as u32;
    if link_count == 255 {
        link_count = cur.read_u32::<LE>().unwrap();
    }

    for _ in 0..link_count {
        let id = read_pano_id(cur);
        let heading = cur.read_f32::<LE>().unwrap();
        let loc = read_location(cur);
        links.push(PanoLink {
            pano: Pano { id, loc },
            heading,
        });
    }

    (this_loc, links.into())
}

pub fn encode_listentityphotos(panos: Option<Arc<[PanoWithBothLocations]>>) -> Vec<u8> {
    let mut buf = Vec::new();

    if let Some(panos) = panos {
        // 1 = normal
        buf.write_u8(1).unwrap();
        for pano in panos.iter() {
            write_pano_id(&mut buf, &pano.id);
            write_location(&mut buf, pano.search_loc);
            write_location(&mut buf, pano.actual_loc);
        }
    } else {
        // 0 = too big, smaller pano should be checked
        buf.write_u8(0).unwrap();
    }

    buf
}
pub fn decode_listentityphotos(cur: &mut Cursor<&[u8]>) -> Option<Arc<[PanoWithBothLocations]>> {
    let mut panos = Vec::new();

    let header = cur.read_u8().unwrap();
    if header == 0 {
        return None;
    }

    while cur.position() < cur.get_ref().len() as u64 {
        let id = read_pano_id(cur);
        let search_loc = read_location(cur);
        let actual_loc = read_location(cur);
        let pano = PanoWithBothLocations {
            id,
            search_loc,
            actual_loc,
        };
        panos.push(pano);
    }

    Some(panos.into())
}

fn write_pano_id(buf: &mut Vec<u8>, pano_id: &PanoId) {
    buf.write_u32::<LE>(pano_id.0).unwrap();
}
fn read_pano_id(cur: &mut Cursor<&[u8]>) -> PanoId {
    PanoId(cur.read_u32::<LE>().unwrap())
}

fn write_location(buf: &mut Vec<u8>, loc: Location) {
    buf.write_i32::<LE>(loc.lat.to_bits()).unwrap();
    buf.write_i32::<LE>(loc.lng.to_bits()).unwrap();
}
fn read_location(cur: &mut Cursor<&[u8]>) -> Location {
    let lat = Angle::from_bits(cur.read_i32::<LE>().unwrap());
    let lng = Angle::from_bits(cur.read_i32::<LE>().unwrap());
    Location { lat, lng }
}

impl BytesEncode<'_> for SizedTile {
    type EItem = SizedTile;
    fn bytes_encode(item: &Self::EItem) -> Result<Cow<'_, [u8]>, BoxedError> {
        let mut buf = Vec::with_capacity(1 + 4 + 4);
        buf.push(item.size);
        buf.write_u32::<LE>(item.x)?;
        buf.write_u32::<LE>(item.y)?;
        Ok(buf.into())
    }
}
impl BytesDecode<'_> for SizedTile {
    type DItem = SizedTile;
    fn bytes_decode(mut bytes: &'_ [u8]) -> Result<Self::DItem, BoxedError> {
        let size = bytes.read_u8()?;
        let x = bytes.read_u32::<LE>()?;
        let y = bytes.read_u32::<LE>()?;
        Ok(SizedTile { size, x, y })
    }
}
