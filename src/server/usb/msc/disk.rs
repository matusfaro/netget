//! Virtual disk image management for USB Mass Storage Class
//!
//! This module provides sector-based read/write operations for virtual disk images
//! using memory-mapped I/O for performance.

#[cfg(feature = "usb-msc")]
use anyhow::{Context, Result};
#[cfg(feature = "usb-msc")]
use memmap2::MmapMut;
#[cfg(feature = "usb-msc")]
use std::fs::{File, OpenOptions};
#[cfg(feature = "usb-msc")]
use std::path::Path;
#[cfg(feature = "usb-msc")]
use tracing::{debug, info, trace};

/// Virtual disk image with memory-mapped I/O
#[cfg(feature = "usb-msc")]
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
    /// # Arguments
    /// * `path` - Path to disk image file
    /// * `size_mb` - Size in megabytes (only used if creating new)
    ///
    /// # Returns
    /// DiskImage instance with memory-mapped file
    pub fn open_or_create(path: &Path, size_mb: u32) -> Result<Self> {
        let bytes_per_sector = 512;
        let size_bytes = (size_mb as u64) * 1024 * 1024;
        let total_sectors = (size_bytes / bytes_per_sector as u64) as u32;

        // Open or create file
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(path)
            .context("Failed to open/create disk image file")?;

        // Ensure file is the correct size
        let current_size = file.metadata()?.len();
        if current_size != size_bytes {
            file.set_len(size_bytes)
                .context("Failed to set disk image size")?;
            info!(
                "Created disk image {} ({} MB, {} sectors)",
                path.display(),
                size_mb,
                total_sectors
            );
        } else {
            debug!(
                "Opened existing disk image {} ({} MB, {} sectors)",
                path.display(),
                size_mb,
                total_sectors
            );
        }

        // Memory-map the file
        let mmap = unsafe {
            MmapMut::map_mut(&file).context("Failed to memory-map disk image file")?
        };

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
        if lba + count > self.total_sectors {
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
        let count = ((data.len() as u32) + self.bytes_per_sector - 1) / self.bytes_per_sector;

        if lba + count > self.total_sectors {
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
    pub fn zero_sectors(&mut self, lba: u32, count: u32) -> Result<()> {
        if lba + count > self.total_sectors {
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
        self.mmap.flush().context("Failed to flush zero operation")?;

        Ok(())
    }
}

#[cfg(all(test, feature = "usb-msc"))]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_disk_image_create() -> Result<()> {
        let temp_dir = std::env::temp_dir();
        let path = temp_dir.join("test_disk_create.img");
        let _ = std::fs::remove_file(&path); // Clean up from previous test

        let disk = DiskImage::open_or_create(&path, 1)?; // 1MB disk

        assert_eq!(disk.total_sectors(), 2048); // 1MB / 512 bytes
        assert_eq!(disk.bytes_per_sector(), 512);

        std::fs::remove_file(&path)?;
        Ok(())
    }

    #[test]
    fn test_disk_image_read_write() -> Result<()> {
        let temp_dir = std::env::temp_dir();
        let path = temp_dir.join("test_disk_rw.img");
        let _ = std::fs::remove_file(&path);

        let mut disk = DiskImage::open_or_create(&path, 1)?;

        // Write test data to sector 10
        let test_data = b"Hello, USB MSC!";
        let sectors_written = disk.write_sectors(10, test_data)?;
        assert_eq!(sectors_written, 1);

        // Read it back
        let read_data = disk.read_sectors(10, 1)?;
        assert_eq!(read_data.len(), 512);
        assert_eq!(&read_data[..test_data.len()], test_data);

        std::fs::remove_file(&path)?;
        Ok(())
    }

    #[test]
    fn test_disk_image_bounds_check() -> Result<()> {
        let temp_dir = std::env::temp_dir();
        let path = temp_dir.join("test_disk_bounds.img");
        let _ = std::fs::remove_file(&path);

        let mut disk = DiskImage::open_or_create(&path, 1)?;

        // Try to read beyond bounds
        let result = disk.read_sectors(2048, 1);
        assert!(result.is_err());

        // Try to write beyond bounds
        let result = disk.write_sectors(2048, b"test");
        assert!(result.is_err());

        std::fs::remove_file(&path)?;
        Ok(())
    }

    #[test]
    fn test_disk_image_zero_sectors() -> Result<()> {
        let temp_dir = std::env::temp_dir();
        let path = temp_dir.join("test_disk_zero.img");
        let _ = std::fs::remove_file(&path);

        let mut disk = DiskImage::open_or_create(&path, 1)?;

        // Write some data
        disk.write_sectors(5, b"This should be zeroed")?;

        // Zero it
        disk.zero_sectors(5, 1)?;

        // Read it back
        let data = disk.read_sectors(5, 1)?;
        assert!(data.iter().all(|&b| b == 0));

        std::fs::remove_file(&path)?;
        Ok(())
    }
}
