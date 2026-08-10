//! A deterministic FAT16 volume builder for the USB mass-storage E2E tests.
//!
//! The point of the MSC tests is "pretend to be a USB drive and serve `hello.txt` containing
//! `world`", and the only honest way to assert that is to put a real filesystem on the virtual
//! disk and read the sectors back through SCSI. Building the volume here rather than checking
//! in a binary image keeps the layout visible and the bytes reproducible: every field below is
//! fixed, so the same call always produces the same image.
//!
//! Layout (512-byte sectors, 1 sector per cluster):
//!
//! | LBA | Contents |
//! |---|---|
//! | 0 | Boot sector / BPB |
//! | 1..33 | FAT #1 (32 sectors) |
//! | 33..65 | FAT #2 |
//! | 65..97 | Root directory (512 entries) |
//! | 97.. | Data region; cluster *n* is LBA `97 + n - 2` |
//!
//! 8192 sectors total (4 MiB) gives 8095 data clusters, comfortably inside FAT16's
//! 4085..65525 window — below it the volume would legally be FAT12 and a host would read the
//! FAT with a different entry width.

#![allow(dead_code)]

pub const BYTES_PER_SECTOR: usize = 512;
pub const SECTORS_PER_CLUSTER: u8 = 1;
pub const RESERVED_SECTORS: u16 = 1;
pub const NUM_FATS: u8 = 2;
pub const ROOT_ENTRIES: u16 = 512;
pub const FAT_SECTORS: u16 = 32;
pub const TOTAL_SECTORS: u16 = 8192;

/// Sectors occupied by the root directory: 512 entries of 32 bytes.
pub const ROOT_DIR_SECTORS: u32 = (ROOT_ENTRIES as u32 * 32).div_ceil(BYTES_PER_SECTOR as u32);

/// First sector of the root directory.
pub const ROOT_DIR_LBA: u32 = RESERVED_SECTORS as u32 + (NUM_FATS as u32 * FAT_SECTORS as u32);

/// First sector of the data region, which holds cluster 2.
pub const DATA_START_LBA: u32 = ROOT_DIR_LBA + ROOT_DIR_SECTORS;

/// Where a cluster's data lives.
pub fn cluster_lba(cluster: u16) -> u32 {
    DATA_START_LBA + (cluster as u32 - 2) * SECTORS_PER_CLUSTER as u32
}

/// One file to place in the root directory.
pub struct FatFile<'a> {
    /// 8.3 name, e.g. `hello.txt`. Case is normalised to upper.
    pub name: &'a str,
    pub data: &'a [u8],
}

/// Build a FAT16 volume containing `files` in the root directory.
///
/// Files are allocated consecutive clusters starting at cluster 2, in the order given.
pub fn build_image(files: &[FatFile]) -> Result<Vec<u8>, String> {
    let total_sectors = TOTAL_SECTORS as usize;
    let mut image = vec![0u8; total_sectors * BYTES_PER_SECTOR];

    if files.len() > ROOT_ENTRIES as usize {
        return Err(format!(
            "{} files do not fit in a {}-entry root directory",
            files.len(),
            ROOT_ENTRIES
        ));
    }

    let data_clusters = total_sectors as u32 - DATA_START_LBA;
    // FAT16 is defined by the cluster count, not by anything in the boot sector.
    if !(4085..65525).contains(&data_clusters) {
        return Err(format!(
            "{} data clusters is outside the FAT16 range 4085..65525",
            data_clusters
        ));
    }

    write_boot_sector(&mut image);

    // FAT entries 0 and 1 are reserved: the media descriptor and the end-of-chain marker.
    let mut fat = vec![0u16; (FAT_SECTORS as usize * BYTES_PER_SECTOR) / 2];
    fat[0] = 0xFFF8;
    fat[1] = 0xFFFF;

    let mut next_cluster: u16 = 2;
    for (index, file) in files.iter().enumerate() {
        let name = encode_8_3(file.name)?;

        let clusters_needed = if file.data.is_empty() {
            0u32
        } else {
            (file.data.len() as u32).div_ceil(BYTES_PER_SECTOR as u32 * SECTORS_PER_CLUSTER as u32)
        };
        if clusters_needed as u64 + next_cluster as u64 > data_clusters as u64 + 2 {
            return Err(format!("'{}' does not fit in the data region", file.name));
        }

        let first_cluster = if clusters_needed == 0 {
            0
        } else {
            next_cluster
        };

        // Chain the clusters and copy the data in.
        for n in 0..clusters_needed {
            let cluster = next_cluster;
            let lba = cluster_lba(cluster) as usize;
            let offset = lba * BYTES_PER_SECTOR;
            let start = (n as usize) * BYTES_PER_SECTOR * SECTORS_PER_CLUSTER as usize;
            let end =
                (start + BYTES_PER_SECTOR * SECTORS_PER_CLUSTER as usize).min(file.data.len());
            let chunk = &file.data[start..end];
            image
                .get_mut(offset..offset + chunk.len())
                .ok_or_else(|| format!("'{}' runs past the end of the image", file.name))?
                .copy_from_slice(chunk);

            let fat_index = cluster as usize;
            *fat.get_mut(fat_index)
                .ok_or_else(|| format!("cluster {} is outside the FAT", cluster))? =
                if n + 1 == clusters_needed {
                    0xFFFF // end of chain
                } else {
                    cluster + 1
                };
            next_cluster += 1;
        }

        // Root directory entry.
        let entry_offset = ROOT_DIR_LBA as usize * BYTES_PER_SECTOR + index * 32;
        let entry = image
            .get_mut(entry_offset..entry_offset + 32)
            .ok_or("root directory entry is outside the image")?;
        entry[0..11].copy_from_slice(&name);
        entry[11] = 0x20; // archive
        entry[22..24].copy_from_slice(&0u16.to_le_bytes()); // write time 00:00:00
        entry[24..26].copy_from_slice(&0x5821u16.to_le_bytes()); // write date 2024-01-01
        entry[26..28].copy_from_slice(&first_cluster.to_le_bytes());
        entry[28..32].copy_from_slice(&(file.data.len() as u32).to_le_bytes());
    }

    // Both FAT copies.
    let fat_bytes: Vec<u8> = fat.iter().flat_map(|e| e.to_le_bytes()).collect();
    for copy in 0..NUM_FATS as usize {
        let offset = (RESERVED_SECTORS as usize + copy * FAT_SECTORS as usize) * BYTES_PER_SECTOR;
        image
            .get_mut(offset..offset + fat_bytes.len())
            .ok_or("FAT does not fit in the image")?
            .copy_from_slice(&fat_bytes);
    }

    Ok(image)
}

/// Build the volume and write it to `path`.
pub fn write_image(path: &std::path::Path, files: &[FatFile]) -> Result<(), String> {
    let image = build_image(files)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create {}: {}", parent.display(), e))?;
    }
    std::fs::write(path, &image).map_err(|e| format!("failed to write {}: {}", path.display(), e))
}

fn write_boot_sector(image: &mut [u8]) {
    let bs = &mut image[..BYTES_PER_SECTOR];

    bs[0..3].copy_from_slice(&[0xEB, 0x3C, 0x90]); // jmp short / nop
    bs[3..11].copy_from_slice(b"MSWIN4.1"); // OEM name

    // BIOS Parameter Block
    bs[11..13].copy_from_slice(&(BYTES_PER_SECTOR as u16).to_le_bytes());
    bs[13] = SECTORS_PER_CLUSTER;
    bs[14..16].copy_from_slice(&RESERVED_SECTORS.to_le_bytes());
    bs[16] = NUM_FATS;
    bs[17..19].copy_from_slice(&ROOT_ENTRIES.to_le_bytes());
    bs[19..21].copy_from_slice(&TOTAL_SECTORS.to_le_bytes());
    bs[21] = 0xF8; // media descriptor: fixed disk
    bs[22..24].copy_from_slice(&FAT_SECTORS.to_le_bytes());
    bs[24..26].copy_from_slice(&32u16.to_le_bytes()); // sectors per track
    bs[26..28].copy_from_slice(&2u16.to_le_bytes()); // heads
    bs[28..32].copy_from_slice(&0u32.to_le_bytes()); // hidden sectors
    bs[32..36].copy_from_slice(&0u32.to_le_bytes()); // total sectors (32-bit), unused

    // Extended BPB
    bs[36] = 0x80; // drive number
    bs[37] = 0x00; // reserved
    bs[38] = 0x29; // extended boot signature
    bs[39..43].copy_from_slice(&0x1234_5678u32.to_le_bytes()); // volume id, fixed on purpose
    bs[43..54].copy_from_slice(b"NETGET     "); // volume label
    bs[54..62].copy_from_slice(b"FAT16   "); // fs type (advisory)

    bs[510] = 0x55;
    bs[511] = 0xAA;
}

/// Encode a name as an 11-byte 8.3 directory entry name.
fn encode_8_3(name: &str) -> Result<[u8; 11], String> {
    let upper = name.to_ascii_uppercase();
    let (base, ext) = match upper.split_once('.') {
        Some((b, e)) => (b, e),
        None => (upper.as_str(), ""),
    };
    if base.is_empty() || base.len() > 8 || ext.len() > 3 {
        return Err(format!("'{}' is not a valid 8.3 name", name));
    }
    if !base
        .bytes()
        .chain(ext.bytes())
        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || b"$%'-_@~`!(){}^#&".contains(&c))
    {
        return Err(format!(
            "'{}' contains characters a FAT 8.3 name cannot hold",
            name
        ));
    }

    let mut out = [b' '; 11];
    out[..base.len()].copy_from_slice(base.as_bytes());
    out[8..8 + ext.len()].copy_from_slice(ext.as_bytes());
    Ok(out)
}
