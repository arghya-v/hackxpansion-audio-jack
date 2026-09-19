# Hackxpansion Audio Jack
This project adds stereo audio playback through a **TLV320DAC3100** DAC and a **3.5 mm headphone jack**, along with physical controls for playback.

## Images
![the case](https://cdn.hackclub.com/01a06d9f-5871-7f07-9ea1-f13b8a1d72d1/image.png)
![pcb](https://cdn.hackclub.com/01a0ba92-1ee8-7d6e-9e94-aa3db825bc52/image.png)
![3d](https://cdn.hackclub.com/01a06db8-5b80-7b92-82bc-98aca223c75d/image.png)

## Schematic
![final schematic](https://cdn.hackclub.com/01a0ba93-0267-7a70-81d2-2f9d5cec6c4d/image.png)

## Features

* Stereo audio output through a 3.5 mm headphone jack
* TLV320DAC3100 stereo DAC
* I²S audio data interface
* 12.288 MHz MCLK generation using RP2350 PIO
* 48 kHz, 16-bit audio
* DMA-backed audio streaming
* Three physical playback controls:

  * **Next**
  * **Previous**
  * **Play / Pause**
* Xpanse module integration through `xpanse-api`
* Built with Embassy for asynchronous embedded Rust

## Hardware

### Audio Module

| GPIO  | Function         |
| ----- | ---------------- |
| GPIO0 | I²C SCL          |
| GPIO1 | I²C SDA          |
| GPIO2 | I²S BCLK         |
| GPIO3 | I²S DIN          |
| GPIO4 | I²S WCLK / LRCLK |
| GPIO5 | DAC MCLK         |
| GPIO6 | DAC RESET        |
| GPIO7 | Next             |
| GPIO8 | Previous         |
| GPIO9 | Play / Pause     |

### Audio Chain


The TLV320DAC3100 provides the stereo DAC and integrated headphone output stage, so no separate headphone amplifier is required for the current design.

### `audio.rs`

Contains the Xpanse audio-module driver, including:

* TLV320DAC3100 initialization
* I²C configuration
* PIO MCLK generation
* PIO I²S output
* DMA audio streaming
* Next / Previous / Play-Pause button registration
* Test audio generation

## Planned Architecture

The final player is intended to use the following pipeline:

```text
SD Card
   │
   ▼
MP3 File
   │
   ▼
MP3 Decoder
   │
   ▼
PCM Audio Buffer
   │
   ▼
DMA
   │
   ▼
I²S / RP2350 PIO
   │
   ▼
TLV320DAC3100
   │
   ▼
3.5 mm Headphone Jack
```
