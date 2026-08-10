//! Virtual disk image management for USB Mass Storage Class
//!
//! This module provides sector-based read/write operations for virtual disk images
//! using memory-mapped I/O for performance.

#[cfg(feature = "usb-msc")]
use anyhow::{Context, Result};
#[cfg(feature = "usb-msc")]
use memmap2::MmapMut;
#[cfg(feature = "usb-msc")]
use std::fs::OpenOptions;
#[cfg(feature = "usb-msc")]
use std::path::Path;
#[cfg(feature = "usb-msc")]
use tracing::{debug, info, trace};

/// Virtual disk image with memory-mapped I/O
#[cfg(feature = "usb-msc")]
#[derive(Debug)]
pub struct DiskImage {
    /// Memory-mapped file for fast sector access
    mmap: MmapMut,
    /// Total number of sectors
    total_sectors: u32,
    /// Bytes per sector (typically 512)
    bytes_per_sector: u32,
}

#[cfg(feature = "usb-msc")]
impl DiskImage {
    /// Open existing disk image or create new one
    ///
    /// An image that already exists keeps its own size: `default_size_mb` only decides how
    /// large a *newly created* image is. Resizing to a fixed default would silently truncate
    /// or pad a filesystem image the caller supplied, which is exactly the case that matters
    /// (`startup_params.disk_image` pointing at a prepared FAT volume).
    ///
    /// # Arguments
    /// * `path` - Path to disk image file
    /// * `default_size_mb` - Size in megabytes, used only when creating a new image
    ///
    /// # Returns
    /// DiskImage instance with memory-mapped file
    pub fn open_or_create(path: &Path, default_size_mb: u32) -> Result<Self> {
        let bytes_per_sector: u32 = 512;

        // Open or create file
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            // Explicit: this opens an existing disk image or creates a new one.
            // Truncating would destroy the image's contents, and leaving it
            // unstated is what clippy's suspicious_open_options flags.
            .truncate(false)
            .open(path)
            .context("Failed to open/create disk image file")?;

        let current_size = file.metadata()?.len();
        let size_bytes = if current_size == 0 {
            // Brand-new (or empty) image: use the requested default size.
            let size_bytes = (default_size_mb as u64) * 1024 * 1024;
            file.set_len(size_bytes)
                .context("Failed to set disk image size")?;
            info!(
                "Created disk image {} ({} MB)",
                path.display(),
                default_size_mb
            );
            size_bytes
        } else {
            // Existing image: keep its contents, but round the mapping up to a whole
            // sector so a partial trailing sector cannot make read_sectors slice past
            // the end of the mapping.
            let rounded = current_size.div_ceil(bytes_per_sector as u64) * bytes_per_sector as u64;
            if rounded != current_size {
                file.set_len(rounded)
                    .context("Failed to pad disk image to a sector boundary")?;
            }
            debug!(
                "Opened existing disk image {} ({} bytes)",
                path.display(),
                rounded
            );
            rounded
        };

        let total_sectors = (size_bytes / bytes_per_sector as u64) as u32;
        if total_sectors == 0 {
            anyhow::bail!(
                "Disk image {} is smaller than one 512-byte sector",
                path.display()
            );
        }

        // Memory-map the file
        let mmap =
            unsafe { MmapMut::map_mut(&file).context("Failed to memory-map disk image file")? };

        Ok(Self {
            mmap,
            total_sectors,
            bytes_per_sector,
        })
    }

    /// Get total number of sectors
    pub fn total_sectors(&self) -> u32 {
        self.total_sectors
    }

    /// Get bytes per sector
    pub fn bytes_per_sector(&self) -> u32 {
        self.bytes_per_sector
    }

    /// Read sectors from disk image
    ///
    /// # Arguments
    /// * `lba` - Logical Block Address (sector number)
    /// * `count` - Number of sectors to read
    ///
    /// # Returns
    /// Vector containing sector data
    pub fn read_sectors(&self, lba: u32, count: u32) -> Result<Vec<u8>> {
        // `lba + count` is host-controlled, so it must not be allowed to wrap: a wrapped sum
        // would compare small and then slice out of bounds.
        let end = lba
            .checked_add(count)
            .context("Read beyond disk bounds: LBA + count overflows")?;
        if end > self.total_sectors {
            anyhow::bail!(
                "Read beyond disk bounds: LBA {} + {} > {}",
                lba,
                count,
                self.total_sectors
            );
        }

        let offset = (lba * self.bytes_per_sector) as usize;
        let length = (count * self.bytes_per_sector) as usize;

        trace!(
            "Reading {} sectors from LBA {} (offset {}, length {})",
            count,
            lba,
            offset,
            length
        );

        Ok(self.mmap[offset..offset + length].to_vec())
    }

    /// Write sectors to disk image
    ///
    /// # Arguments
    /// * `lba` - Logical Block Address (sector number)
    /// * `data` - Data to write (will be padded to sector boundary if needed)
    ///
    /// # Returns
    /// Number of sectors written
    pub fn write_sectors(&mut self, lba: u32, data: &[u8]) -> Result<u32> {
        let count = u32::try_from(data.len().div_ceil(self.bytes_per_sector as usize))
            .context("Write payload is too large to address in sectors")?;

        let end = lba
            .checked_add(count)
            .context("Write beyond disk bounds: LBA + count overflows")?;
        if end > self.total_sectors {
            anyhow::bail!(
                "Write beyond disk bounds: LBA {} + {} > {}",
                lba,
                count,
                self.total_sectors
            );
        }

        let offset = (lba * self.bytes_per_sector) as usize;
        let length = data.len();

        trace!(
            "Writing {} bytes ({} sectors) to LBA {} (offset {})",
            length,
            count,
            lba,
            offset
        );

        self.mmap[offset..offset + length].copy_from_slice(data);

        // Flush to disk
        self.mmap.flush().context("Failed to flush disk writes")?;

        Ok(count)
    }

    /// Zero out a range of sectors
    ///
    /// # Arguments
    /// * `lba` - Starting sector
    /// * `count` - Number of sectors to zero
    #[allow(dead_code)]
    pub fn zero_sectors(&mut self, lba: u32, count: u32) -> Result<()> {
        let end = lba
            .checked_add(count)
            .context("Zero beyond disk bounds: LBA + count overflows")?;
        if end > self.total_sectors {
            anyhow::bail!(
                "Zero beyond disk bounds: LBA {} + {} > {}",
                lba,
                count,
                self.total_sectors
            );
        }

        let offset = (lba * self.bytes_per_sector) as usize;
        let length = (count * self.bytes_per_sector) as usize;

        debug!("Zeroing {} sectors from LBA {}", count, lba);

        self.mmap[offset..offset + length].fill(0);
        self.mmap
            .flush()
            .context("Failed to flush zero operation")?;

        Ok(())
    }
}
