# Third-Party Licenses

Terminal Studio is licensed under Apache-2.0.  
The following direct dependencies are bundled or linked into the binary; their licenses are reproduced or summarized below.

| Crate | Version | License |
|-------|---------|---------|
| alacritty_terminal | 0.26.x | Apache-2.0 |
| anyhow | 1.x | MIT OR Apache-2.0 |
| eframe | 0.28.x | MIT |
| egui | 0.28.x | MIT |
| env_logger | 0.11.x | MIT OR Apache-2.0 |
| log | 0.4.x | MIT OR Apache-2.0 |
| notify | 6.x | MIT |
| parking_lot | 0.12.x | MIT OR Apache-2.0 |
| portable-pty | 0.8.x | MIT |
| serde | 1.x | MIT OR Apache-2.0 |
| serde_json | 1.x | MIT OR Apache-2.0 |
| vte | 0.13.x | MIT OR Apache-2.0 |
| windows-sys | 0.52.x | MIT OR Apache-2.0 |

Each crate's full license text is available in the Cargo registry cache and on crates.io.

---

## alacritty_terminal

Copyright 2016 Joe Wilm  
Copyright 2019 The Alacritty Project Contributors

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

    http://www.apache.org/licenses/LICENSE-2.0

---

## eframe / egui

Copyright 2018 Emil Ernerfeldt

MIT License:  
Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the "Software"), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

---

## portable-pty

Copyright 2019 Wez Furlong

MIT License (see above MIT text).

---

## vte

Copyright 2016 Joe Wilm

MIT OR Apache-2.0 (dual-licensed).

---

## All other crates (anyhow, env_logger, log, notify, parking_lot, serde, serde_json, windows-sys)

These crates are licensed under either MIT or Apache-2.0 at the user's option.  
Full license texts for MIT and Apache-2.0 are available at:

- MIT: https://opensource.org/licenses/MIT  
- Apache-2.0: https://www.apache.org/licenses/LICENSE-2.0

---

To regenerate this list from the current lockfile, run:

```
cargo install cargo-license
cargo license
```
