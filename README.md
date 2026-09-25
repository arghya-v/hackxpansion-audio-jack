# Hackxpansion Audio Jack
This project adds stereo audio playback through a **TLV320DAC3100** DAC and a **3.5 mm headphone jack**, along with physical controls for playback.
<br/>
**Crates**: pkg:cargo/audio_jack_firmware@0.1.0
## Images
![the case](https://cdn.hackclub.com/01a06d9f-5871-7f07-9ea1-f13b8a1d72d1/image.png)
![pcb](https://cdn.hackclub.com/01a0bb2e-7094-7034-8e83-c10658a84b9e/image.png)
![3d](https://cdn.hackclub.com/01a06db8-5b80-7b92-82bc-98aca223c75d/image.png)

## Schematic
![final schematic](https://cdn.hackclub.com/01a0bb2d-8350-7ae1-a54f-9b21094ca6af/image.png)

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

See the [Xpanse API docs](https://docs.rs/xpanse-api/latest/xpanse_api/index.html)


---

This was all possible thanks to [Hackspansion: A hackclub YSWS](http://hackxpansion.hackclub.com/)
