use embassy_executor::Spawner;

use embassy_rp::{
    bind_interrupts,
    dma,
    gpio::{Level, Output},
    i2c,
    peripherals::{DMA_CH0, I2C0},
    pio_programs::{
        clk::{PioClk, PioClkProgram},
        i2s::{PioI2sOut, PioI2sOutProgram},
    },
};

use embedded_hal_async::i2c::I2c;

use xpanse_api::{
    with_pio,
    bus::allocator::BusAllocator,
    driver::{Driver, DriverError, DriverMeta},
    gpio_bank::{BankPins, GpioBank},
    interfaces::buttons::{
        pin_button,
        A,
        B,
        X,
    },
    metadata::{ModuleDetectResistor, ModuleID, ModuleSlot},
    registry::Registry,
};

const DAC_ADDRESS: u8 = 0x18;

const SAMPLE_RATE: u32 = 48_000;
const BIT_DEPTH: u32 = 16;
const MCLK_FREQUENCY: u32 = 12_288_000;

bind_interrupts!(struct I2cIrqs {
    I2C0_IRQ => embassy_rp::i2c::InterruptHandler<I2C0>;
});

bind_interrupts!(struct AudioDmaIrqs {
    DMA_IRQ_0 => dma::InterruptHandler<DMA_CH0>;
});

pub struct AudioDriver;

impl DriverMeta for AudioDriver {
    const ID: ModuleID = ModuleID {
        md0: ModuleDetectResistor::R1K,
        md1: ModuleDetectResistor::R62K,
    };
}

impl<G> Driver<G> for AudioDriver
where
    G: BankPins,
    G::GPIO0: embassy_rp::i2c::SclPin<I2C0>,
    G::GPIO1: embassy_rp::i2c::SdaPin<I2C0>,
{
    async fn create(
        bank: GpioBank<G>,
        slot: ModuleSlot,
        registry: &mut Registry,
        buses: &mut BusAllocator,
    ) -> Result<(), DriverError> {
        // -------------------------------------------------
        // GPIO MAP
        //
        // GPIO0 = I2C SCL
        // GPIO1 = I2C SDA
        // GPIO2 = I2S BCLK
        // GPIO3 = I2S DIN
        // GPIO4 = I2S WCLK / LRCLK
        // GPIO5 = MCLK
        // GPIO6 = DAC RESET
        // GPIO7 = NEXT
        // GPIO8 = PREVIOUS
        // GPIO9 = PLAY / PAUSE
        // -------------------------------------------------

        // -------------------------------------------------
        // DAC HARDWARE RESET
        // -------------------------------------------------

        let mut reset = Output::new(
            bank.gpio6,
            Level::Low,
        );

        embassy_time::Timer::after_millis(5).await;

        reset.set_high();

        embassy_time::Timer::after_millis(10).await;

        // Keep RESET asserted high for the lifetime
        // of the driver.
        core::mem::forget(reset);

        // -------------------------------------------------
        // I2C
        // -------------------------------------------------

        let mut i2c_bus = buses
            .create_i2c_hardware::<I2C0, _>(
                bank.gpio0,
                bank.gpio1,
                I2cIrqs,
                i2c::Config::default(),
            )
            .map_err(|_| DriverError::InitFailed)?;

        // -------------------------------------------------
        // DAC SOFTWARE RESET
        // -------------------------------------------------

        dac_write(
            &mut i2c_bus,
            0x00,
            0x01,
        )
        .await?;

        embassy_time::Timer::after_millis(10).await;

        // -------------------------------------------------
        // DAC CONFIGURATION
        // -------------------------------------------------

        configure_dac(&mut i2c_bus).await?;

        // -------------------------------------------------
        // BUTTONS
        //
        // A = NEXT
        // B = PREVIOUS
        // C = PLAY / PAUSE
        // -------------------------------------------------

        registry.register(
            slot,
            Self::ID,
            pin_button::<A>(bank.gpio7.into()),
        );

        registry.register(
            slot,
            Self::ID,
            pin_button::<B>(bank.gpio8.into()),
        );

        registry.register(
            slot,
            Self::ID,
            pin_button::<X>(bank.gpio9.into()),
        );

        // -------------------------------------------------
        // DMA
        // -------------------------------------------------

        let dma = buses
            .request_dma::<DMA_CH0>()
            .map_err(|_| DriverError::InitFailed)?;

        // -------------------------------------------------
        // SPAWNER
        // -------------------------------------------------

        let spawner = unsafe {
            Spawner::for_current_executor().await
        };

        // -------------------------------------------------
        // MCLK PIO
        //
        // IMPORTANT:
        //
        // We configure the first PIO completely before
        // requesting another PIO from BusAllocator.
        //
        // This avoids borrowing `buses` twice at once.
        // -------------------------------------------------

        let mclk_pio = buses
            .request_pio(&[
                &bank.gpio5,
            ])
            .ok_or(DriverError::InitFailed)?;

        with_pio!(
            mclk_pio,
            mclk_common,
            mclk_sm,
            {
                let mclk_program =
                    PioClkProgram::new(
                        &mut *mclk_common,
                    );

                let mut mclk = PioClk::new(
                    &mut *mclk_common,
                    mclk_sm,
                    bank.gpio5,
                    &mclk_program,
                    MCLK_FREQUENCY,
                );

                // Start 12.288 MHz master clock.
                mclk.start();

                // MCLK must continue running for the
                // lifetime of the audio system.
                core::mem::forget(mclk);
            }
        );

        // -------------------------------------------------
        // I2S PIO
        //
        // The previous PioAccess has now gone out of scope,
        // so `buses` can be borrowed again.
        // -------------------------------------------------

        let i2s_pio = buses
            .request_pio(&[
                &bank.gpio2,
                &bank.gpio3,
                &bank.gpio4,
            ])
            .ok_or(DriverError::InitFailed)?;

        with_pio!(
            i2s_pio,
            i2s_common,
            i2s_sm,
            {
                let i2s_program =
                    PioI2sOutProgram::new(
                        &mut *i2s_common,
                    );

                let mut i2s = PioI2sOut::new(
                    &mut *i2s_common,
                    i2s_sm,
                    dma,
                    AudioDmaIrqs,
                    bank.gpio3, // DIN
                    bank.gpio2, // BCLK
                    bank.gpio4, // LRCLK
                    SAMPLE_RATE,
                    BIT_DEPTH,
                    &i2s_program,
                );

                // Start I2S.
                i2s.start();

                // `with_pio!` has recovered the concrete
                // PIO/SM type, so this selects the correct
                // Embassy task automatically.
                i2s.spawn_audio_task(spawner);
            }
        );

        Ok(())
    }
}

// =========================================================
// AUDIO TASK DISPATCH
// =========================================================
//
// PioI2sOut is parameterized by the concrete PIO block and
// state-machine number.
//
// Xpanse's with_pio! macro recovers those concrete types.
//
// This trait maps each possible type to its Embassy task.
//
// =========================================================

trait SpawnAudioTask {
    fn spawn_audio_task(
        self,
        spawner: Spawner,
    );
}

// ---------------------------------------------------------
// PIO0
// ---------------------------------------------------------

impl SpawnAudioTask
    for PioI2sOut<
        'static,
        embassy_rp::peripherals::PIO0,
        0,
    >
{
    fn spawn_audio_task(
        self,
        spawner: Spawner,
    ) {
        spawner
            .spawn(audio_task_pio0_sm0(self).unwrap());
    }
}

impl SpawnAudioTask
    for PioI2sOut<
        'static,
        embassy_rp::peripherals::PIO0,
        1,
    >
{
    fn spawn_audio_task(
        self,
        spawner: Spawner,
    ) {
        spawner
            .spawn(audio_task_pio0_sm1(self).unwrap());
    }
}

impl SpawnAudioTask
    for PioI2sOut<
        'static,
        embassy_rp::peripherals::PIO0,
        2,
    >
{
    fn spawn_audio_task(
        self,
        spawner: Spawner,
    ) {
        spawner
            .spawn(audio_task_pio0_sm2(self).unwrap());
    }
}

impl SpawnAudioTask
    for PioI2sOut<
        'static,
        embassy_rp::peripherals::PIO0,
        3,
    >
{
    fn spawn_audio_task(
        self,
        spawner: Spawner,
    ) {
        spawner
            .spawn(audio_task_pio0_sm3(self).unwrap());
    }
}

// ---------------------------------------------------------
// PIO1
// ---------------------------------------------------------

impl SpawnAudioTask
    for PioI2sOut<
        'static,
        embassy_rp::peripherals::PIO1,
        0,
    >
{
    fn spawn_audio_task(
        self,
        spawner: Spawner,
    ) {
        spawner
            .spawn(audio_task_pio1_sm0(self).unwrap());
    }
}

impl SpawnAudioTask
    for PioI2sOut<
        'static,
        embassy_rp::peripherals::PIO1,
        1,
    >
{
    fn spawn_audio_task(
        self,
        spawner: Spawner,
    ) {
        spawner
            .spawn(audio_task_pio1_sm1(self).unwrap());
    }
}

impl SpawnAudioTask
    for PioI2sOut<
        'static,
        embassy_rp::peripherals::PIO1,
        2,
    >
{
    fn spawn_audio_task(
        self,
        spawner: Spawner,
    ) {
        spawner
            .spawn(audio_task_pio1_sm2(self).unwrap());
    }
}

impl SpawnAudioTask
    for PioI2sOut<
        'static,
        embassy_rp::peripherals::PIO1,
        3,
    >
{
    fn spawn_audio_task(
        self,
        spawner: Spawner,
    ) {
        spawner
            .spawn(audio_task_pio1_sm3(self).unwrap());
    }
}

// ---------------------------------------------------------
// PIO2
// ---------------------------------------------------------

impl SpawnAudioTask
    for PioI2sOut<
        'static,
        embassy_rp::peripherals::PIO2,
        0,
    >
{
    fn spawn_audio_task(
        self,
        spawner: Spawner,
    ) {
        spawner
            .spawn(audio_task_pio2_sm0(self).unwrap());
    }
}

impl SpawnAudioTask
    for PioI2sOut<
        'static,
        embassy_rp::peripherals::PIO2,
        1,
    >
{
    fn spawn_audio_task(
        self,
        spawner: Spawner,
    ) {
        spawner
            .spawn(audio_task_pio2_sm1(self).unwrap());
    }
}

impl SpawnAudioTask
    for PioI2sOut<
        'static,
        embassy_rp::peripherals::PIO2,
        2,
    >
{
    fn spawn_audio_task(
        self,
        spawner: Spawner,
    ) {
        spawner
            .spawn(audio_task_pio2_sm2(self).unwrap());
    }
}

impl SpawnAudioTask
    for PioI2sOut<
        'static,
        embassy_rp::peripherals::PIO2,
        3,
    >
{
    fn spawn_audio_task(
        self,
        spawner: Spawner,
    ) {
        spawner
            .spawn(audio_task_pio2_sm3(self).unwrap());
    }
}

// =========================================================
// PIO0 TASKS
// =========================================================

#[embassy_executor::task]
async fn audio_task_pio0_sm0(
    i2s: PioI2sOut<
        'static,
        embassy_rp::peripherals::PIO0,
        0,
    >,
) {
    audio_task_loop(i2s).await;
}

#[embassy_executor::task]
async fn audio_task_pio0_sm1(
    i2s: PioI2sOut<
        'static,
        embassy_rp::peripherals::PIO0,
        1,
    >,
) {
    audio_task_loop(i2s).await;
}

#[embassy_executor::task]
async fn audio_task_pio0_sm2(
    i2s: PioI2sOut<
        'static,
        embassy_rp::peripherals::PIO0,
        2,
    >,
) {
    audio_task_loop(i2s).await;
}

#[embassy_executor::task]
async fn audio_task_pio0_sm3(
    i2s: PioI2sOut<
        'static,
        embassy_rp::peripherals::PIO0,
        3,
    >,
) {
    audio_task_loop(i2s).await;
}

// =========================================================
// PIO1 TASKS
// =========================================================

#[embassy_executor::task]
async fn audio_task_pio1_sm0(
    i2s: PioI2sOut<
        'static,
        embassy_rp::peripherals::PIO1,
        0,
    >,
) {
    audio_task_loop(i2s).await;
}

#[embassy_executor::task]
async fn audio_task_pio1_sm1(
    i2s: PioI2sOut<
        'static,
        embassy_rp::peripherals::PIO1,
        1,
    >,
) {
    audio_task_loop(i2s).await;
}

#[embassy_executor::task]
async fn audio_task_pio1_sm2(
    i2s: PioI2sOut<
        'static,
        embassy_rp::peripherals::PIO1,
        2,
    >,
) {
    audio_task_loop(i2s).await;
}

#[embassy_executor::task]
async fn audio_task_pio1_sm3(
    i2s: PioI2sOut<
        'static,
        embassy_rp::peripherals::PIO1,
        3,
    >,
) {
    audio_task_loop(i2s).await;
}

// =========================================================
// PIO2 TASKS
// =========================================================

#[embassy_executor::task]
async fn audio_task_pio2_sm0(
    i2s: PioI2sOut<
        'static,
        embassy_rp::peripherals::PIO2,
        0,
    >,
) {
    audio_task_loop(i2s).await;
}

#[embassy_executor::task]
async fn audio_task_pio2_sm1(
    i2s: PioI2sOut<
        'static,
        embassy_rp::peripherals::PIO2,
        1,
    >,
) {
    audio_task_loop(i2s).await;
}

#[embassy_executor::task]
async fn audio_task_pio2_sm2(
    i2s: PioI2sOut<
        'static,
        embassy_rp::peripherals::PIO2,
        2,
    >,
) {
    audio_task_loop(i2s).await;
}

#[embassy_executor::task]
async fn audio_task_pio2_sm3(
    i2s: PioI2sOut<
        'static,
        embassy_rp::peripherals::PIO2,
        3,
    >,
) {
    audio_task_loop(i2s).await;
}

// =========================================================
// AUDIO TEST LOOP
// =========================================================

async fn audio_task_loop<P, const SM: usize>(
    mut i2s: PioI2sOut<'static, P, SM>,
)
where
    P: embassy_rp::pio::Instance,
{
    let mut buffer = [0u32; 256];

    let mut phase: u32 = 0;

    loop {
        for word in &mut buffer {
            let sample: i16 = if phase < 32_768 {
                12_000
            } else {
                -12_000
            };

            let sample = sample as u16 as u32;

            // Stereo:
            //
            // [ LEFT ][ RIGHT ]
            // [15:0]  [15:0]
            //
            *word = (sample << 16) | sample;

            phase = phase.wrapping_add(1_000);
        }

        i2s.write(&buffer).await;
    }
}

// =========================================================
// DAC I2C WRITE
// =========================================================

async fn dac_write<I>(
    i2c: &mut I,
    register: u8,
    value: u8,
) -> Result<(), DriverError>
where
    I: I2c,
{
    i2c.write(
        DAC_ADDRESS,
        &[register, value],
    )
    .await
    .map_err(|_| DriverError::InitFailed)
}

// =========================================================
// TLV320DAC3100 CONFIGURATION
// =========================================================

async fn configure_dac<I>(
    i2c: &mut I,
) -> Result<(), DriverError>
where
    I: I2c,
{
    // -------------------------------------------------
    // PAGE 0
    // -------------------------------------------------

    dac_write(i2c, 0x00, 0x00).await?;

    // CODEC_CLKIN = MCLK
    dac_write(i2c, 0x04, 0x00).await?;

    // NDAC = 8, powered
    dac_write(i2c, 0x0B, 0x88).await?;

    // MDAC = 2, powered
    dac_write(i2c, 0x0C, 0x82).await?;

    // DOSR = 128
    dac_write(i2c, 0x0D, 0x00).await?;
    dac_write(i2c, 0x0E, 0x80).await?;

    // I2S, 16-bit, codec slave
    dac_write(i2c, 0x1B, 0x00).await?;

    // -------------------------------------------------
    // PAGE 1
    // -------------------------------------------------

    dac_write(i2c, 0x00, 0x01).await?;

    // Common-mode voltage
    dac_write(i2c, 0x1F, 0x04).await?;

    // De-pop
    dac_write(i2c, 0x21, 0x4E).await?;

    // LDAC -> HPL
    // RDAC -> HPR
    dac_write(i2c, 0x23, 0x44).await?;

    // HPL unmuted
    dac_write(i2c, 0x28, 0x06).await?;

    // HPR unmuted
    dac_write(i2c, 0x29, 0x06).await?;

    // Power HPL + HPR
    dac_write(i2c, 0x1F, 0xC2).await?;

    // Analog output volume = -9 dB
    dac_write(i2c, 0x24, 0x92).await?;
    dac_write(i2c, 0x25, 0x92).await?;

    // -------------------------------------------------
    // PAGE 0
    // -------------------------------------------------

    dac_write(i2c, 0x00, 0x00).await?;

    // Power DAC L/R + soft stepping
    dac_write(i2c, 0x3F, 0xD4).await?;

    // Digital gain
    dac_write(i2c, 0x41, 0xD4).await?;
    dac_write(i2c, 0x42, 0xD4).await?;

    // Unmute DAC
    dac_write(i2c, 0x40, 0x00).await?;

    Ok(())
}