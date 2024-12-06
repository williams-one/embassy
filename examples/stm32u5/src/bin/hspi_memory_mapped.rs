#![no_main]
#![no_std]

// Tested on an STM32U5G9J-DK2 demo board using the on-board MX66LM1G45G flash memory
// The flash is connected to the HSPI1 port as an OCTOSPI device
//
// Use embassy-stm32 feature "stm32u5g9zj" and probe-rs chip "STM32U5G9ZJTxQ"

use defmt::info;
use embassy_executor::Spawner;
use embassy_stm32::hspi::{
    AddressSize, ChipSelectHighTime, DummyCycles, FIFOThresholdLevel, Hspi, HspiWidth, Instance, MemorySize,
    MemoryType, TransferConfig, WrapSize,
};
// use embassy_stm32::gpio::{Level, Output, Speed};
use embassy_stm32::mode::Blocking;
use embassy_stm32::time::Hertz;
use embassy_stm32::{pac, rcc, Config};
// use embassy_time::Timer;
use {defmt_rtt as _, panic_probe as _};

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    // RCC config
    let mut config = Config::default();
    info!("START");
    config.rcc.hse = Some(rcc::Hse {
        freq: Hertz(16_000_000),
        mode: rcc::HseMode::Oscillator,
    });
    config.rcc.pll1 = Some(rcc::Pll {
        source: rcc::PllSource::HSE,
        prediv: rcc::PllPreDiv::DIV1,
        mul: rcc::PllMul::MUL10,
        divp: None,
        divq: None,
        divr: Some(rcc::PllDiv::DIV1),
    });
    config.rcc.sys = rcc::Sysclk::PLL1_R; // 160 Mhz
    config.rcc.pll2 = Some(rcc::Pll {
        source: rcc::PllSource::HSE,
        prediv: rcc::PllPreDiv::DIV4,
        mul: rcc::PllMul::MUL50, // MUL66: Running at full speed causes read/write errors!!
        divp: None,
        divq: Some(rcc::PllDiv::DIV2),
        divr: None,
    });
    config.rcc.mux.hspi1sel = rcc::mux::Hspisel::PLL2_Q; // 100 //132 MHz

    // Initialize peripherals
    let p = embassy_stm32::init(config);

    let ospi_config = embassy_stm32::hspi::Config {
        fifo_threshold: FIFOThresholdLevel::_4Bytes,
        memory_type: MemoryType::Macronix,
        device_size: MemorySize::_1GiB,
        chip_select_high_time: ChipSelectHighTime::_2Cycle,
        free_running_clock: false,
        clock_mode: false,
        wrap_size: WrapSize::None,
        clock_prescaler: 0,
        sample_shifting: false,
        delay_hold_quarter_cycle: false,
        chip_select_boundary: 0,
        delay_block_bypass: false,
        max_transfer: 0,
        refresh: 0,
    };
    let hspi = embassy_stm32::hspi::Hspi::new_blocking_octospi(
        p.HSPI1,
        p.PI3,
        p.PH10,
        p.PH11,
        p.PH12,
        p.PH13,
        p.PH14,
        p.PH15,
        p.PI0,
        p.PI1,
        p.PH9,
        ospi_config,
    );

    info!("HSPI initialized");

    let mut flash = FlashMemory::new(hspi).await;

    let flash_id = flash.read_id();
    info!("FLASH ID: {=[u8]:x}", flash_id);
    let mut wr_buf = [0u8; 16];
    for i in 0..wr_buf.len() {
        wr_buf[i] = (i + 1) as u8;
    }
    let mut rd_buf = [0u8; 16];
    embassy_time::Timer::after_millis(100).await;
    flash.erase_sector(0).await;
    flash.write_memory(0, &wr_buf, true).await;
    flash.read_memory(0, &mut rd_buf, false);
    info!("WRITE BUF: {=[u8]:#X}", wr_buf);
    info!("READ BUF: {=[u8]:#X}", rd_buf);

    flash.enable_mm().await;
    info!("Enabled memory mapped mode");

    let first_u32 = unsafe { *(0xA0000000 as *const u32) };
    info!("first_u32: 0x{=u32:X}", first_u32);
    // assert_eq!(first_u32, 0x03020100);

    let second_u32 = unsafe { *(0xA0000004 as *const u32) };
    // assert_eq!(second_u32, 0x07060504);
    info!("second_u32: 0x{=u32:X}", second_u32);
    // flash.disable_mm().await;

    info!("DONE");
    //     // Output pin PE3
    //     let mut led = Output::new(p.PE3, Level::Low, Speed::Low);

    //     loop {
    //         led.toggle();
    //         Timer::after_millis(1000).await;
    //     }
}

// TODO(willy) capire come gestire fallimento/timeout delle richieste!!

/// Custom implementation for MX66UW1G45G NOR flash memory from Macronix.
/// Chip commands are hardcoded as they depend on the chip used.
/// This implementation enables Octa I/O (OPI) and Double Transfer Rate (DTR)
pub struct FlashMemory<I: Instance> {
    hspi: Hspi<'static, I, Blocking>,
}

impl<I: Instance> FlashMemory<I> {
    const MEMORY_PAGE_SIZE: usize = 256;

    const CMD_READ: u8 = 0x03;
    // const CMD_QUAD_READ: u8 = 0x6B;

    const CMD_PAGE_PROGRAM: u8 = 0x02;

    // const CMD_QUAD_WRITE_PG: u8 = 0x32;

    const CMD_READ_ID: u8 = 0x9F;
    const CMD_READ_ID_DOPI: u16 = 0x9F60;
    const CMD_OCTA_READ_DTR: u16 = 0xEE11;

    const CMD_RESET_ENABLE: u8 = 0x66;
    const CMD_RESET: u8 = 0x99;

    const CMD_WRITE_ENABLE: u8 = 0x06;

    // const CMD_CHIP_ERASE: u8 = 0xC7;
    const CMD_SECTOR_ERASE: u8 = 0x20;
    // const CMD_BLOCK_ERASE_32K: u8 = 0x52;
    // const CMD_BLOCK_ERASE_64K: u8 = 0xD8;

    const CMD_READ_SR: u8 = 0x05;

    const CMD_READ_CR2: u8 = 0x71;
    const CMD_WRITE_CR2: u8 = 0x72;

    // const CMD_WRITE_SR: u8 = 0x01;
    // const CMD_WRITE_CR: u8 = 0x31;

    const CR2_REG1_ADDR: u32 = 0x00000000;
    const CR2_DOPI: u8 = 0x02;

    const CR2_REG3_ADDR: u32 = 0x00000300;
    const CR2_DC_6_CYCLES: u8 = 0x06;

    pub async fn new(hspi: Hspi<'static, I, Blocking>) -> Self {
        let mut memory = Self { hspi };

        memory.reset_memory().await;
        // memory.enable_dtr_opi().await;
        memory
    }

    async fn enable_dtr_opi(&mut self) {
        self.write_enable().await;
        self.write_cr2(Self::CR2_REG3_ADDR, Self::CR2_DC_6_CYCLES);
        self.write_enable().await;
        self.write_cr2(Self::CR2_REG1_ADDR, Self::CR2_DOPI);
    }

    //     pub async fn disable_mm(&mut self) {
    //         self.ospi.disable_memory_mapped_mode();
    //     }

    pub async fn enable_mm(&mut self) {
        // In teoria non e' necessario
        // self.qpi_mode().await;

        let read_config = TransferConfig {
            iwidth: HspiWidth::OCTO,
            instruction: Some(Self::CMD_OCTA_READ_DTR as u32),
            isize: AddressSize::_16Bit,
            idtr: true,
            adwidth: HspiWidth::OCTO,
            adsize: AddressSize::_32Bit,
            addtr: true,
            dwidth: HspiWidth::OCTO,
            ddtr: true,
            ..Default::default()
        };

        let write_config = TransferConfig {
            // iwidth: OspiWidth::SING,
            // isize: AddressSize::_8Bit,
            // adwidth: OspiWidth::SING,
            // adsize: AddressSize::_24bit,
            // dwidth: OspiWidth::QUAD,
            // instruction: Some(0x32), // Write config
            // dummy: DummyCycles::_0,
            ..Default::default()
        };
        self.hspi.enable_memory_mapped_mode(read_config, write_config).unwrap();
    }

    //     fn enable_quad(&mut self) {
    //         let cr = self.read_cr();
    //         // info!("Read cr: {:x}", cr);
    //         self.write_cr(cr | 0x02);
    //         // info!("Read cr after writing: {:x}", cr);
    //     }

    //     pub fn disable_quad(&mut self) {
    //         let cr = self.read_cr();
    //         self.write_cr(cr & (!(0x02)));
    //     }

    // async fn exec_command_4(&mut self, cmd: u8) {
    // let transaction = TransferConfig {
    //     iwidth: HspiWidth::OCTO,
    //     adwidth: OspiWidth::NONE,
    //     // adsize: AddressSize::_24bit,
    //     dwidth: OspiWidth::NONE,
    //     instruction: Some(cmd as u32),
    //     address: None,
    //     dummy: DummyCycles::_0,
    //     ..Default::default()
    // };
    // self.ospi.command(&transaction).await.unwrap();
    // }

    async fn exec_command(&mut self, cmd: u8) {
        let transaction = TransferConfig {
            iwidth: HspiWidth::SING,
            instruction: Some(cmd as u32),
            ..Default::default()
        };
        info!("Excuting command: 0x{:X}", transaction.instruction.unwrap());
        self.hspi.command(&transaction).await.unwrap();
    }

    pub async fn reset_memory(&mut self) {
        // servono entrambi i comandi?
        // self.exec_command_4(Self::CMD_RESET_ENABLE).await;
        // self.exec_command_4(Self::CMD_RESET).await;
        self.exec_command(Self::CMD_RESET_ENABLE).await;
        self.exec_command(Self::CMD_RESET).await;
        self.wait_write_finish();
    }

    pub async fn write_enable(&mut self) {
        self.exec_command(Self::CMD_WRITE_ENABLE).await;
    }

    pub fn read_id(&mut self) -> [u8; 3] {
        let mut buffer = [0; 3];
        let transaction: TransferConfig = TransferConfig {
            iwidth: HspiWidth::SING,
            instruction: Some(Self::CMD_READ_ID as u32),
            dwidth: HspiWidth::SING,
            ..Default::default()
        };
        info!("Reading flash id: 0x{:X}", transaction.instruction.unwrap());
        self.hspi.blocking_read(&mut buffer, transaction).unwrap();
        buffer
    }

    pub fn read_id_dopi(&mut self) -> [u8; 3] {
        let mut buffer = [0; 3];
        let transaction: TransferConfig = TransferConfig {
            iwidth: HspiWidth::OCTO,
            instruction: Some(Self::CMD_READ_ID_DOPI as u32),
            isize: AddressSize::_16Bit,
            idtr: true,
            adwidth: HspiWidth::OCTO,
            address: Some(0),
            adsize: AddressSize::_32Bit,
            addtr: true,
            dwidth: HspiWidth::OCTO,
            ddtr: true,
            ..Default::default()
        };
        info!("Reading flash id: 0x{:X}", transaction.instruction.unwrap());
        self.hspi.blocking_read(&mut buffer, transaction).unwrap();
        buffer
    }

    //     pub fn read_id_4(&mut self) -> [u8; 3] {
    //         let mut buffer = [0; 3];
    //         let transaction: TransferConfig = TransferConfig {
    //             iwidth: OspiWidth::SING,
    //             isize: AddressSize::_8Bit,
    //             adwidth: OspiWidth::NONE,
    //             dwidth: OspiWidth::QUAD,
    //             instruction: Some(Self::CMD_READ_ID as u32),
    //             ..Default::default()
    //         };
    //         info!("Reading id: 0x{:X}", transaction.instruction);
    //         self.ospi.blocking_read(&mut buffer, transaction).unwrap();
    //         buffer
    //     }

    pub fn read_memory(&mut self, addr: u32, buffer: &mut [u8], use_dma: bool) {
        let transaction = TransferConfig {
            iwidth: HspiWidth::SING,
            instruction: Some(Self::CMD_READ as u32),
            adwidth: HspiWidth::SING,
            address: Some(addr),
            adsize: AddressSize::_24Bit,
            dwidth: HspiWidth::SING,
            ..Default::default()
        };
        if use_dma {
            // DMA is not yet supported
            self.hspi.blocking_read(buffer, transaction).unwrap();
        } else {
            self.hspi.blocking_read(buffer, transaction).unwrap();
        }
    }

    // in OCTO mode
    // pub fn read_memory(&mut self, addr: u32, buffer: &mut [u8], use_dma: bool) {
    //     let transaction = TransferConfig {
    //         iwidth: OspiWidth::SING,
    //         adwidth: OspiWidth::SING,
    //         adsize: AddressSize::_24bit,
    //         dwidth: OspiWidth::QUAD,
    //         instruction: Some(Self::CMD_QUAD_READ as u32),
    //         address: Some(addr),
    //         dummy: DummyCycles::_8,
    //         ..Default::default()
    //     };
    //     if use_dma {
    //         // DMA is not yet supported
    //         self.ospi.blocking_read(buffer, transaction).unwrap();
    //     } else {
    //         self.ospi.blocking_read(buffer, transaction).unwrap();
    //     }
    // }

    fn wait_write_finish(&mut self) {
        while (self.read_sr() & 0x01) != 0 {}
    }

    async fn perform_erase(&mut self, addr: u32, cmd: u8) {
        let transaction = TransferConfig {
            iwidth: HspiWidth::SING,
            instruction: Some(cmd as u32),
            adwidth: HspiWidth::SING,
            address: Some(addr),
            adsize: AddressSize::_24Bit,
            ..Default::default()
        };
        self.write_enable().await;
        self.hspi.command(&transaction).await.unwrap();
        self.wait_write_finish();
        info!("Erase operation completed");
    }

    pub async fn erase_sector(&mut self, addr: u32) {
        info!("Erasing 4K sector at address: 0x{:X}", addr);
        self.perform_erase(addr, Self::CMD_SECTOR_ERASE).await;
    }

    //     pub async fn erase_block_32k(&mut self, addr: u32) {
    //         self.perform_erase(addr, CMD_BLOCK_ERASE_32K).await;
    //     }

    //     pub async fn erase_block_64k(&mut self, addr: u32) {
    //         self.perform_erase(addr, CMD_BLOCK_ERASE_64K).await;
    //     }

    //     pub async fn erase_chip(&mut self) {
    //         self.exec_command(Self::CMD_CHIP_ERASE).await;
    //     }

    async fn write_page(&mut self, addr: u32, buffer: &[u8], len: usize, use_dma: bool) {
        assert!(
            (len as u32 + (addr & 0x000000ff)) <= Self::MEMORY_PAGE_SIZE as u32,
            "write_page(): page write length exceeds page boundary (len = {}, addr = {:X}",
            len,
            addr
        );

        let transaction = TransferConfig {
            iwidth: HspiWidth::SING,
            instruction: Some(Self::CMD_PAGE_PROGRAM as u32),
            adwidth: HspiWidth::SING,
            address: Some(addr),
            adsize: AddressSize::_24Bit,
            dwidth: HspiWidth::SING,
            ..Default::default()
        };
        self.write_enable().await;
        if use_dma {
            self.hspi.blocking_write(buffer, transaction).unwrap();
        } else {
            self.hspi.blocking_write(buffer, transaction).unwrap();
        }
        self.wait_write_finish();
    }

    pub async fn write_memory(&mut self, addr: u32, buffer: &[u8], use_dma: bool) {
        let mut left = buffer.len();
        let mut place = addr;
        let mut chunk_start = 0;

        while left > 0 {
            let max_chunk_size = Self::MEMORY_PAGE_SIZE - (place & 0x000000ff) as usize;
            let chunk_size = if left >= max_chunk_size { max_chunk_size } else { left };
            let chunk = &buffer[chunk_start..(chunk_start + chunk_size)];
            self.write_page(place, chunk, chunk_size, use_dma).await;
            place += chunk_size as u32;
            left -= chunk_size;
            chunk_start += chunk_size;
        }
    }

    // write flash in octo mode
    // async fn write_page(&mut self, addr: u32, buffer: &[u8], len: usize, use_dma: bool) {
    //     assert!(
    //         (len as u32 + (addr & 0x000000ff)) <= MEMORY_PAGE_SIZE as u32,
    //         "write_page(): page write length exceeds page boundary (len = {}, addr = {:X}",
    //         len,
    //         addr
    //     );

    //     let transaction = TransferConfig {
    //         iwidth: OspiWidth::SING,
    //         adsize: AddressSize::_24bit,
    //         adwidth: OspiWidth::SING,
    //         dwidth: OspiWidth::QUAD,
    //         instruction: Some(Self::CMD_QUAD_WRITE_PG as u32),
    //         address: Some(addr),
    //         dummy: DummyCycles::_0,
    //         ..Default::default()
    //     };
    //     self.write_enable().await;
    //     if use_dma {
    //         self.ospi.blocking_write(buffer, transaction).unwrap();
    //     } else {
    //         self.ospi.blocking_write(buffer, transaction).unwrap();
    //     }
    //     self.wait_write_finish();
    // }

    // pub async fn write_memory(&mut self, addr: u32, buffer: &[u8], use_dma: bool) {
    //     let mut left = buffer.len();
    //     let mut place = addr;
    //     let mut chunk_start = 0;

    //     while left > 0 {
    //         let max_chunk_size = MEMORY_PAGE_SIZE - (place & 0x000000ff) as usize;
    //         let chunk_size = if left >= max_chunk_size { max_chunk_size } else { left };
    //         let chunk = &buffer[chunk_start..(chunk_start + chunk_size)];
    //         self.write_page(place, chunk, chunk_size, use_dma).await;
    //         place += chunk_size as u32;
    //         left -= chunk_size;
    //         chunk_start += chunk_size;
    //     }
    // }

    // fn read_register(&mut self, cmd: u8) -> u8 {
    //     let mut buffer = [0; 1];
    //     let transaction: TransferConfig = TransferConfig {
    //         iwidth: HspiWidth::SING,
    //         instruction: Some(cmd as u32),
    //         dwidth: HspiWidth::SING,
    //         ..Default::default()
    //     };
    //     self.hspi.blocking_read(&mut buffer, transaction).unwrap();
    //     // info!("Read MX66LM1G45G register: 0x{:x}", buffer[0]);
    //     buffer[0]
    // }

    // fn write_register(&mut self, cmd: u8, value: u8) {
    //     let buffer = [value; 1];
    //     let transaction: TransferConfig = TransferConfig {
    //         iwidth: HspiWidth::SING,
    //         isize: AddressSize::_8Bit,
    //         instruction: Some(cmd as u32),
    //         adsize: AddressSize::_24bit,
    //         adwidth: HspiWidth::NONE,
    //         dwidth: HspiWidth::SING,
    //         address: None,
    //         dummy: DummyCycles::_0,
    //         ..Default::default()
    //     };
    //     self.hspi.blocking_write(&buffer, transaction).unwrap();
    // }

    pub fn read_sr(&mut self) -> u8 {
        let mut buffer = [0; 1];
        let transaction: TransferConfig = TransferConfig {
            iwidth: HspiWidth::SING,
            instruction: Some(Self::CMD_READ_SR as u32),
            dwidth: HspiWidth::SING,
            ..Default::default()
        };
        self.hspi.blocking_read(&mut buffer, transaction).unwrap();
        // info!("Read MX66LM1G45G SR register: 0x{:x}", buffer[0]);
        buffer[0]
    }

    pub fn read_cr2(&mut self, addr: u32) -> u8 {
        let mut buffer = [0; 1];
        let transaction: TransferConfig = TransferConfig {
            iwidth: HspiWidth::SING,
            instruction: Some(Self::CMD_READ_CR2 as u32),
            adwidth: HspiWidth::SING,
            address: Some(addr),
            adsize: AddressSize::_32Bit,
            dwidth: HspiWidth::SING,
            ..Default::default()
        };
        self.hspi.blocking_read(&mut buffer, transaction).unwrap();
        info!("Read MX66LM1G45G CR2[0x{:X}] register: 0x{:x}", addr, buffer[0]);
        buffer[0]
    }

    //     pub fn write_sr(&mut self, value: u8) {
    //         self.write_register(Self::CMD_WRITE_SR, value);
    //     }

    pub fn write_cr2(&mut self, addr: u32, value: u8) {
        let buffer = [value; 1];
        let transaction: TransferConfig = TransferConfig {
            iwidth: HspiWidth::SING,
            instruction: Some(Self::CMD_WRITE_CR2 as u32),
            adwidth: HspiWidth::SING,
            address: Some(addr),
            adsize: AddressSize::_32Bit,
            dwidth: HspiWidth::SING,
            ..Default::default()
        };
        self.hspi.blocking_write(&buffer, transaction).unwrap();
    }
}
