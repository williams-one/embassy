#![no_main]
#![no_std]

// Tested on an STM32U5G9J-DK2 demo board using the on-board MX66LM1G45G flash memory
// The flash is connected to the HSPI1 port as an OCTOSPI device
//
// Use embassy-stm32 feature "stm32u5g9zj" and probe-rs chip "STM32U5G9ZJTxQ"

use core::hint::black_box;

use defmt::info;
use embassy_executor::Spawner;
use embassy_stm32::hspi::{
    AddressSize, ChipSelectHighTime, FIFOThresholdLevel, Hspi, HspiWidth, Instance, MemorySize, MemoryType,
    TransferConfig, WrapSize,
};
use embassy_stm32::mode::Blocking;
use embassy_stm32::time::Hertz;
use embassy_stm32::{rcc, Config};
use embassy_time::Instant;
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
        mul: rcc::PllMul::MUL33, // MUL66: Running at full speed causes read/write errors!!
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
        p.PI2,
        ospi_config,
    );

    info!("HSPI initialized");

    let mut flash: FlashMemory<embassy_stm32::peripherals::HSPI1> = FlashMemory::new(hspi).await;
    flash.enable_mm().await;

    flash_benchmark();

    info!("DONE");
}

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
        memory.enable_dtr_opi().await;
        memory
    }

    async fn enable_dtr_opi(&mut self) {
        self.write_enable().await;
        self.write_cr2(Self::CR2_REG3_ADDR, Self::CR2_DC_6_CYCLES);
        self.write_enable().await;
        self.write_cr2(Self::CR2_REG1_ADDR, Self::CR2_DOPI);
    }

    pub async fn enable_mm(&mut self) {
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
            iwidth: HspiWidth::OCTO,
            isize: AddressSize::_16Bit,
            idtr: true,
            adwidth: HspiWidth::OCTO,
            adsize: AddressSize::_32Bit,
            addtr: true,
            dwidth: HspiWidth::OCTO,
            ddtr: true,
            ..Default::default()
        };
        self.hspi.enable_memory_mapped_mode(read_config, write_config).unwrap();
    }

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
            // self.hspi.blocking_read_dma(buffer, transaction).unwrap();
        } else {
            self.hspi.blocking_read(buffer, transaction).unwrap();
        }
    }

    pub fn read_memory_dopi(&mut self, addr: u32, buffer: &mut [u8], use_dma: bool) {
        let transaction = TransferConfig {
            iwidth: HspiWidth::OCTO,
            instruction: Some(Self::CMD_OCTA_READ_DTR as u32),
            isize: AddressSize::_16Bit,
            idtr: true,
            adwidth: HspiWidth::OCTO,
            address: Some(addr),
            adsize: AddressSize::_32Bit,
            addtr: true,
            dwidth: HspiWidth::OCTO,
            ddtr: true,
            ..Default::default()
        };
        if use_dma {
            // self.hspi.blocking_read_dma(buffer, transaction).unwrap();
        } else {
            self.hspi.blocking_read(buffer, transaction).unwrap();
        }
    }

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
            // self.hspi.blocking_write_dma(buffer, transaction).unwrap();
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

use static_cell::StaticCell;

const VAR_SIZE_POWER_OF_TWO: u32 = 18;
const VAR_SIZE: usize = 2usize.pow(VAR_SIZE_POWER_OF_TWO);

static FLASH_VAR: [u8; VAR_SIZE] = [1u8; VAR_SIZE];

#[used]
#[link_section = ".ExtFlashSection"]
static EXT_FLASH_VAR: [u8; VAR_SIZE] = [1u8; VAR_SIZE];

static RAM_VAR: StaticCell<heapless::Vec<u8, VAR_SIZE>> = StaticCell::new();
static BUFFER: StaticCell<heapless::Vec<u8, VAR_SIZE>> = StaticCell::new();

fn fill_sawtooth(var: &mut [u8]) {
    for (i, x) in var.iter_mut().enumerate() {
        *x = i as u8;
    }
}

fn check_sawtooth(var: &[u8]) {
    for (i, x) in var.iter().enumerate() {
        if *x != i as u8 {
            panic!("var[{}] contains a wrong value!", i);
        }
    }
}

fn check_all_one(var: &[u8]) {
    for (i, x) in var.iter().enumerate() {
        if *x != 1 as u8 {
            panic!("var[{}] contains a wrong value!", i);
        }
    }
}

fn check_memory_sizes(src: &[u8], dst: &[u8]) {
    if src.len() != dst.len() {
        panic!("src and dst must have the same length!");
    }
}

fn check_block_size(var: &[u8], block_size: usize) {
    if (var.len() % block_size) != 0 {
        panic!(
            "var length {} must be a multiple of BLOCK_SIZE {}!",
            var.len(),
            block_size
        );
    }
}

fn unsafe_copy(src: &[u8], dst: &mut [u8], block_size: usize) {
    check_memory_sizes(src, dst);
    check_block_size(src, block_size);

    for i in 0..dst.len() / block_size {
        unsafe {
            let dst_ptr = &mut dst[i * block_size] as *mut u8;
            let src_ptr = &src[i * block_size] as *const u8;
            core::ptr::copy_nonoverlapping(src_ptr, dst_ptr, block_size);
        }
    }
}

pub fn time_operation<F>(mut benchmark: F) -> u64
where
    F: FnMut(),
{
    let start_time = Instant::now();
    benchmark();
    start_time.elapsed().as_micros()
}

macro_rules! run_benchmark {
    ($label:expr, $function:expr) => {
        info!(
            "{}: {} us",
            $label,
            time_operation(|| {
                $function;
            })
        );
    };
}

fn flash_benchmark() {
    info!("Starting flash benchmark");
    let mut buffer = BUFFER.init_with(|| heapless::Vec::new());
    buffer.resize(VAR_SIZE, 1).expect("Cannot resize heapless vec");

    let mut ram_var = RAM_VAR.init_with(|| heapless::Vec::new());
    ram_var.resize(VAR_SIZE, 1).expect("Cannot resize heapless vec");
    fill_sawtooth(&mut ram_var);

    let block_size = 2usize.pow(VAR_SIZE_POWER_OF_TWO);

    // Read benchmark
    run_benchmark!("RAM", unsafe_copy(black_box(&ram_var), &mut buffer, block_size));
    check_sawtooth(&buffer);
    run_benchmark!("FLASH", unsafe_copy(&FLASH_VAR, &mut buffer, block_size));
    check_all_one(&buffer);
    run_benchmark!(
        "EXT_FLASH",
        unsafe_copy(black_box(&EXT_FLASH_VAR), &mut buffer, block_size)
    );
    check_all_one(&buffer);
}
