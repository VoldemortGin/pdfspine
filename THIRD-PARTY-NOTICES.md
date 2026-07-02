# Third-Party Notices — pdfspine

pdfspine 的二进制 / wheel 分发物包含以下第三方开源组件。各组件版权归其各自作者所有，
按其各自许可证授权。**全部为宽松许可（MIT / Apache-2.0 / BSD / Zlib / 0BSD / Unlicense /
Unicode-3.0 / BlueOak），无 copyleft（GPL / AGPL / LGPL / MPL）组件。** 本文件满足这些
许可证的归因（attribution）义务，须随 pdfspine 二进制 / wheel 一并分发。

第三方组件总数：**186**（覆盖含 OCR 的完整 wheel）。

> 本文件由仓库脚本从 `cargo license` 元数据生成。多许可 `OR` 表达式表示该组件
> 可在所列任一许可证下使用；相关许可证全文见文末。Apache-2.0 全文见随附 LICENSE。

---

## 组件清单

| 组件 | 版本 | 许可证 | 版权 / 作者 | 仓库 |
|---|---|---|---|---|
| adler2 | 2.0.1 | 0BSD OR Apache-2.0 OR MIT | Jonas Schievink <jonasschievink@gmail.com>, oyvindln <oyvind | https://github.com/oyvindln/adler2 |
| aes | 0.8.4 | Apache-2.0 OR MIT | RustCrypto Developers | https://github.com/RustCrypto/block-ciphers |
| ahash | 0.8.12 | Apache-2.0 OR MIT | Tom Kaitchuck <Tom.Kaitchuck@gmail.com> | https://github.com/tkaitchuck/ahash |
| aho-corasick | 1.1.4 | MIT OR Unlicense | Andrew Gallant <jamslam@gmail.com> | https://github.com/BurntSushi/aho-corasick |
| anyhow | 1.0.102 | Apache-2.0 OR MIT | David Tolnay <dtolnay@gmail.com> | https://github.com/dtolnay/anyhow |
| anymap2 | 0.13.0 | Apache-2.0 OR MIT | Chris Morgan <me@chrismorgan.info>, Azriel Hoh <azriel91@gma | https://github.com/azriel91/anymap2 |
| anymap3 | 1.1.0 | Apache-2.0 OR BlueOak-1.0.0 OR MIT | Olivier 'reivilibre' (fork maintainer) <contact@librepush.ne | https://github.com/reivilibre/anymap3 |
| arrayref | 0.3.9 | BSD-2-Clause | David Roundy <roundyd@physics.oregonstate.edu> | https://github.com/droundy/arrayref |
| arrayvec | 0.7.6 | Apache-2.0 OR MIT | bluss | https://github.com/bluss/arrayvec |
| autocfg | 1.5.1 | Apache-2.0 OR MIT | Josh Stone <cuviper@gmail.com> | https://github.com/cuviper/autocfg |
| bit-set | 0.5.3 | Apache-2.0 OR MIT | Alexis Beingessner <a.beingessner@gmail.com> | https://github.com/contain-rs/bit-set |
| bit-vec | 0.6.3 | Apache-2.0 OR MIT | Alexis Beingessner <a.beingessner@gmail.com> | https://github.com/contain-rs/bit-vec |
| bitflags | 2.13.0 | Apache-2.0 OR MIT | The Rust Project Developers | https://github.com/bitflags/bitflags |
| block-buffer | 0.10.4 | Apache-2.0 OR MIT | RustCrypto Developers | https://github.com/RustCrypto/utils |
| block-padding | 0.3.3 | Apache-2.0 OR MIT | RustCrypto Developers | https://github.com/RustCrypto/utils |
| borsh | 1.6.1 | Apache-2.0 OR MIT | Near Inc <hello@near.org> | https://github.com/near/borsh-rs |
| bytemuck | 1.25.0 | Apache-2.0 OR MIT OR Zlib | Lokathor <zefria@gmail.com> | https://github.com/Lokathor/bytemuck |
| byteorder | 1.5.0 | MIT OR Unlicense | Andrew Gallant <jamslam@gmail.com> | https://github.com/BurntSushi/byteorder |
| byteorder-lite | 0.1.0 | MIT OR Unlicense |  | https://github.com/image-rs/byteorder-lite |
| bytes | 1.11.1 | MIT | Carl Lerche <me@carllerche.com>, Sean McArthur <sean@seanmon | https://github.com/tokio-rs/bytes |
| cbc | 0.1.2 | Apache-2.0 OR MIT | RustCrypto Developers | https://github.com/RustCrypto/block-modes |
| cc | 1.2.64 | Apache-2.0 OR MIT | Alex Crichton <alex@alexcrichton.com> | https://github.com/rust-lang/cc-rs |
| cfg-if | 1.0.4 | Apache-2.0 OR MIT | Alex Crichton <alex@alexcrichton.com> | https://github.com/rust-lang/cfg-if |
| cfg_aliases | 0.2.1 | MIT | Zicklag <zicklag@katharostech.com> | https://github.com/katharostech/cfg_aliases |
| cipher | 0.4.4 | Apache-2.0 OR MIT | RustCrypto Developers | https://github.com/RustCrypto/traits |
| color_quant | 1.1.0 | MIT | nwin <nwin@users.noreply.github.com> | https://github.com/image-rs/color_quant.git |
| cpufeatures | 0.2.17 | Apache-2.0 OR MIT | RustCrypto Developers | https://github.com/RustCrypto/utils |
| crc32fast | 1.5.0 | Apache-2.0 OR MIT | Sam Rijs <srijs@airpost.net>, Alex Crichton <alex@alexcricht | https://github.com/srijs/rust-crc32fast |
| crossbeam-deque | 0.8.6 | Apache-2.0 OR MIT |  | https://github.com/crossbeam-rs/crossbeam |
| crossbeam-epoch | 0.9.18 | Apache-2.0 OR MIT |  | https://github.com/crossbeam-rs/crossbeam |
| crossbeam-utils | 0.8.21 | Apache-2.0 OR MIT |  | https://github.com/crossbeam-rs/crossbeam |
| crunchy | 0.2.4 | MIT | Eira Fransham <jackefransham@gmail.com> | https://github.com/eira-fransham/crunchy |
| crypto-common | 0.1.7 | Apache-2.0 OR MIT | RustCrypto Developers | https://github.com/RustCrypto/traits |
| deranged | 0.5.8 | Apache-2.0 OR MIT | Jacob Pratt <jacob@jhpratt.dev> | https://github.com/jhpratt/deranged |
| derive-new | 0.5.9 | MIT | Nick Cameron <ncameron@mozilla.com> | https://github.com/nrc/derive-new |
| digest | 0.10.7 | Apache-2.0 OR MIT | RustCrypto Developers | https://github.com/RustCrypto/traits |
| doc-comment | 0.3.4 | MIT | Guillaume Gomez <guillaume1.gomez@gmail.com> | https://github.com/GuillaumeGomez/doc-comment |
| downcast-rs | 1.2.1 | Apache-2.0 OR MIT | Ashish Myles <marcianx@gmail.com>, Runji Wang <wangrunji0408 | https://github.com/marcianx/downcast-rs |
| dyn-clone | 1.0.20 | Apache-2.0 OR MIT | David Tolnay <dtolnay@gmail.com> | https://github.com/dtolnay/dyn-clone |
| dyn-hash | 0.2.2 | Apache-2.0 OR MIT | David Tolnay <dtolnay@gmail.com> | https://github.com/dtolnay/dyn-hash |
| either | 1.16.0 | Apache-2.0 OR MIT |  | https://github.com/rayon-rs/either |
| errno | 0.3.14 | Apache-2.0 OR MIT | Chris Wong <lambda.fairy@gmail.com>, Dan Gohman <dev@sunfish | https://github.com/lambda-fairy/rust-errno |
| fax | 0.2.7 | MIT | Sebastian K <s3bk@protonmail.com> | https://github.com/pdf-rs/fax |
| fdeflate | 0.3.7 | Apache-2.0 OR MIT | The image-rs Developers | https://github.com/image-rs/fdeflate |
| fearless_simd | 0.4.1 | Apache-2.0 OR MIT | Raph Levien <raph.levien@gmail.com> | https://github.com/linebender/fearless_simd |
| filetime | 0.2.29 | Apache-2.0 OR MIT | Alex Crichton <alex@alexcrichton.com> | https://github.com/alexcrichton/filetime |
| find-msvc-tools | 0.1.9 | Apache-2.0 OR MIT |  | https://github.com/rust-lang/cc-rs |
| flate2 | 1.1.9 | Apache-2.0 OR MIT | Alex Crichton <alex@alexcrichton.com>, Josh Triplett <josh@j | https://github.com/rust-lang/flate2-rs |
| generic-array | 0.14.7 | MIT | Bartłomiej Kamiński <fizyk20@gmail.com>, Aaron Trent <novacr | https://github.com/fizyk20/generic-array.git |
| getrandom | 0.2.17 | Apache-2.0 OR MIT | The Rand Project Developers | https://github.com/rust-random/getrandom |
| gif | 0.14.2 | Apache-2.0 OR MIT | The image-rs Developers | https://github.com/image-rs/image-gif |
| half | 2.7.1 | Apache-2.0 OR MIT | Kathryn Long <squeeself@gmail.com> | https://github.com/VoidStarKat/half-rs |
| hashbrown | 0.14.5 | Apache-2.0 OR MIT | Amanieu d'Antras <amanieu@gmail.com> | https://github.com/rust-lang/hashbrown |
| hayro-ccitt | 0.3.0 | Apache-2.0 OR MIT | Laurenz Stampfl <laurenz.stampfl@gmail.com> | https://github.com/LaurenzV/hayro |
| hayro-jbig2 | 0.3.0 | Apache-2.0 OR MIT | Laurenz Stampfl <laurenz.stampfl@gmail.com> | https://github.com/LaurenzV/hayro |
| hayro-jpeg2000 | 0.4.0 | Apache-2.0 OR MIT | Laurenz Stampfl <laurenz.stampfl@gmail.com> | https://github.com/LaurenzV/hayro |
| heck | 0.5.0 | Apache-2.0 OR MIT |  | https://github.com/withoutboats/heck |
| image | 0.25.10 | Apache-2.0 OR MIT | The image-rs Developers | https://github.com/image-rs/image |
| image-webp | 0.2.4 | Apache-2.0 OR MIT |  | https://github.com/image-rs/image-webp |
| inout | 0.1.4 | Apache-2.0 OR MIT | RustCrypto Developers | https://github.com/RustCrypto/utils |
| itertools | 0.10.5 | Apache-2.0 OR MIT | bluss | https://github.com/rust-itertools/itertools |
| itertools | 0.12.1 | Apache-2.0 OR MIT | bluss | https://github.com/rust-itertools/itertools |
| itertools | 0.13.0 | Apache-2.0 OR MIT | bluss | https://github.com/rust-itertools/itertools |
| jpeg-decoder | 0.3.2 | Apache-2.0 OR MIT | The image-rs Developers | https://github.com/image-rs/jpeg-decoder |
| kstring | 2.0.2 | Apache-2.0 OR MIT | Ed Page <eopage@gmail.com> | https://github.com/cobalt-org/kstring |
| lazy_static | 1.5.0 | Apache-2.0 OR MIT | Marvin Löbel <loebel.marvin@gmail.com> | https://github.com/rust-lang-nursery/lazy-static.rs |
| libc | 0.2.186 | Apache-2.0 OR MIT | The Rust Project Developers | https://github.com/rust-lang/libc |
| libm | 0.2.16 | MIT | Alex Crichton <alex@alexcrichton.com>, Amanieu d'Antras <ama | https://github.com/rust-lang/compiler-builtins |
| linux-raw-sys | 0.12.1 | Apache-2.0 OR Apache-2.0 WITH LLVM-exception OR MIT | Dan Gohman <dev@sunfishcode.online> | https://github.com/sunfishcode/linux-raw-sys |
| liquid | 0.26.8 | Apache-2.0 OR MIT |  | https://github.com/cobalt-org/liquid-rust |
| liquid-core | 0.26.8 | Apache-2.0 OR MIT | Ed Page <eopage@gmail.com> | https://github.com/cobalt-org/liquid-rust/tree/master/crate/core |
| liquid-derive | 0.26.8 | Apache-2.0 OR MIT | Pedro Gonçalo Correia <goncalerta@gmail.com> | https://github.com/cobalt-org/liquid-rust |
| liquid-lib | 0.26.8 | Apache-2.0 OR MIT | Johann Hofmann <mail@johann-hofmann.com> | https://github.com/cobalt-org/liquid-rust/tree/master/liquid-lib |
| lock_api | 0.4.14 | Apache-2.0 OR MIT | Amanieu d'Antras <amanieu@gmail.com> | https://github.com/Amanieu/parking_lot |
| log | 0.4.32 | Apache-2.0 OR MIT | The Rust Project Developers | https://github.com/rust-lang/log |
| maplit | 1.0.2 | Apache-2.0 OR MIT | bluss | https://github.com/bluss/maplit |
| matrixmultiply | 0.3.10 | Apache-2.0 OR MIT | bluss, R. Janis Goldschmidt | https://github.com/bluss/matrixmultiply/ |
| md-5 | 0.10.6 | Apache-2.0 OR MIT | RustCrypto Developers | https://github.com/RustCrypto/hashes |
| memchr | 2.8.2 | MIT OR Unlicense | Andrew Gallant <jamslam@gmail.com>, bluss | https://github.com/BurntSushi/memchr |
| memmap2 | 0.9.10 | Apache-2.0 OR MIT | Dan Burkert <dan@danburkert.com>, Yevhenii Reizner <razrfalc | https://github.com/RazrFalcon/memmap2-rs |
| minimal-lexical | 0.2.1 | Apache-2.0 OR MIT | Alex Huszagh <ahuszagh@gmail.com> | https://github.com/Alexhuszagh/minimal-lexical |
| miniz_oxide | 0.8.9 | Apache-2.0 OR MIT OR Zlib | Frommi <daniil.liferenko@gmail.com>, oyvindln <oyvindln@user | https://github.com/Frommi/miniz_oxide/tree/master/miniz_oxide |
| moxcms | 0.8.1 | Apache-2.0 OR BSD-3-Clause | Radzivon Bartoshyk | https://github.com/awxkee/moxcms.git |
| ndarray | 0.16.1 | Apache-2.0 OR MIT | Ulrik Sverdrup "bluss", Jim Turner | https://github.com/rust-ndarray/ndarray |
| nom | 7.1.3 | MIT | contact@geoffroycouprie.com | https://github.com/Geal/nom |
| num-complex | 0.4.6 | Apache-2.0 OR MIT | The Rust Project Developers | https://github.com/rust-num/num-complex |
| num-conv | 0.2.2 | Apache-2.0 OR MIT | Jacob Pratt <jacob@jhpratt.dev> | https://github.com/jhpratt/num-conv |
| num-integer | 0.1.46 | Apache-2.0 OR MIT | The Rust Project Developers | https://github.com/rust-num/num-integer |
| num-traits | 0.2.19 | Apache-2.0 OR MIT | The Rust Project Developers | https://github.com/rust-num/num-traits |
| once_cell | 1.21.4 | Apache-2.0 OR MIT | Aleksey Kladov <aleksey.kladov@gmail.com> | https://github.com/matklad/once_cell |
| parking_lot | 0.12.5 | Apache-2.0 OR MIT | Amanieu d'Antras <amanieu@gmail.com> | https://github.com/Amanieu/parking_lot |
| parking_lot_core | 0.9.12 | Apache-2.0 OR MIT | Amanieu d'Antras <amanieu@gmail.com> | https://github.com/Amanieu/parking_lot |
| paste | 1.0.15 | Apache-2.0 OR MIT | David Tolnay <dtolnay@gmail.com> | https://github.com/dtolnay/paste |
| percent-encoding | 2.3.2 | Apache-2.0 OR MIT | The rust-url developers | https://github.com/servo/rust-url/ |
| pest | 2.8.6 | Apache-2.0 OR MIT | Dragoș Tiselice <dragostiselice@gmail.com> | https://github.com/pest-parser/pest |
| pest_derive | 2.8.6 | Apache-2.0 OR MIT | Dragoș Tiselice <dragostiselice@gmail.com> | https://github.com/pest-parser/pest |
| pest_generator | 2.8.6 | Apache-2.0 OR MIT | Dragoș Tiselice <dragostiselice@gmail.com> | https://github.com/pest-parser/pest |
| pest_meta | 2.8.6 | Apache-2.0 OR MIT | Dragoș Tiselice <dragostiselice@gmail.com> | https://github.com/pest-parser/pest |
| png | 0.18.1 | Apache-2.0 OR MIT | The image-rs Developers | https://github.com/image-rs/image-png |
| portable-atomic | 1.13.1 | Apache-2.0 OR MIT |  | https://github.com/taiki-e/portable-atomic |
| portable-atomic-util | 0.2.7 | Apache-2.0 OR MIT |  | https://github.com/taiki-e/portable-atomic-util |
| powerfmt | 0.2.0 | Apache-2.0 OR MIT | Jacob Pratt <jacob@jhpratt.dev> | https://github.com/jhpratt/powerfmt |
| ppv-lite86 | 0.2.21 | Apache-2.0 OR MIT | The CryptoCorrosion Contributors | https://github.com/cryptocorrosion/cryptocorrosion |
| primal-check | 0.3.4 | Apache-2.0 OR MIT | Huon Wilson <dbau.pp@gmail.com> | https://github.com/huonw/primal |
| proc-macro2 | 1.0.106 | Apache-2.0 OR MIT | David Tolnay <dtolnay@gmail.com>, Alex Crichton <alex@alexcr | https://github.com/dtolnay/proc-macro2 |
| prost | 0.11.9 | Apache-2.0 | Dan Burkert <dan@danburkert.com>, Lucio Franco <luciofranco1 | https://github.com/tokio-rs/prost |
| prost-derive | 0.11.9 | Apache-2.0 | Dan Burkert <dan@danburkert.com>, Lucio Franco <luciofranco1 | https://github.com/tokio-rs/prost |
| pxfm | 0.1.29 | Apache-2.0 OR BSD-3-Clause | Radzivon Bartoshyk | https://github.com/awxkee/pxfm |
| pyo3 | 0.29.0 | Apache-2.0 OR MIT | PyO3 Project and Contributors <https://github.com/PyO3> | https://github.com/pyo3/pyo3 |
| pyo3-build-config | 0.29.0 | Apache-2.0 OR MIT | PyO3 Project and Contributors <https://github.com/PyO3> | https://github.com/pyo3/pyo3 |
| pyo3-ffi | 0.29.0 | Apache-2.0 OR MIT | PyO3 Project and Contributors <https://github.com/PyO3> | https://github.com/pyo3/pyo3 |
| pyo3-macros | 0.29.0 | Apache-2.0 OR MIT | PyO3 Project and Contributors <https://github.com/PyO3> | https://github.com/pyo3/pyo3 |
| pyo3-macros-backend | 0.29.0 | Apache-2.0 OR MIT | PyO3 Project and Contributors <https://github.com/PyO3> | https://github.com/pyo3/pyo3 |
| pulldown-cmark | 0.12.2 | MIT | Raph Levien <raph.levien@gmail.com>, Marcus Klaas de Vries < | https://github.com/raphlinus/pulldown-cmark |
| quick-error | 2.0.1 | Apache-2.0 OR MIT | Paul Colomiets <paul@colomiets.name>, Colin Kiegel <kiegel@g | http://github.com/tailhook/quick-error |
| quote | 1.0.45 | Apache-2.0 OR MIT | David Tolnay <dtolnay@gmail.com> | https://github.com/dtolnay/quote |
| rand | 0.8.6 | Apache-2.0 OR MIT | The Rand Project Developers, The Rust Project Developers | https://github.com/rust-random/rand |
| rand_chacha | 0.3.1 | Apache-2.0 OR MIT | The Rand Project Developers, The Rust Project Developers, Th | https://github.com/rust-random/rand |
| rand_core | 0.6.4 | Apache-2.0 OR MIT | The Rand Project Developers, The Rust Project Developers | https://github.com/rust-random/rand |
| rand_distr | 0.4.3 | Apache-2.0 OR MIT | The Rand Project Developers | https://github.com/rust-random/rand |
| rawpointer | 0.2.1 | Apache-2.0 OR MIT | bluss | https://github.com/bluss/rawpointer/ |
| rayon | 1.12.0 | Apache-2.0 OR MIT |  | https://github.com/rayon-rs/rayon |
| rayon-core | 1.13.0 | Apache-2.0 OR MIT |  | https://github.com/rayon-rs/rayon |
| redox_syscall | 0.5.18 | MIT | Jeremy Soller <jackpot51@gmail.com> | https://gitlab.redox-os.org/redox-os/syscall |
| regex | 1.12.4 | Apache-2.0 OR MIT | The Rust Project Developers, Andrew Gallant <jamslam@gmail.c | https://github.com/rust-lang/regex |
| regex-automata | 0.4.14 | Apache-2.0 OR MIT | The Rust Project Developers, Andrew Gallant <jamslam@gmail.c | https://github.com/rust-lang/regex |
| regex-syntax | 0.8.11 | Apache-2.0 OR MIT | The Rust Project Developers, Andrew Gallant <jamslam@gmail.c | https://github.com/rust-lang/regex |
| rustfft | 6.4.1 | Apache-2.0 OR MIT | Allen Welkie <allen.welkie at gmail>, Elliott Mahler <join.t | https://github.com/ejmahler/RustFFT |
| rustix | 1.1.4 | Apache-2.0 OR Apache-2.0 WITH LLVM-exception OR MIT | Dan Gohman <dev@sunfishcode.online>, Jakub Konka <kubkon@jak | https://github.com/bytecodealliance/rustix |
| same-file | 1.0.6 | MIT OR Unlicense | Andrew Gallant <jamslam@gmail.com> | https://github.com/BurntSushi/same-file |
| scan_fmt | 0.2.6 | MIT | wlentz | https://github.com/wlentz/scan_fmt |
| scopeguard | 1.2.0 | Apache-2.0 OR MIT | bluss | https://github.com/bluss/scopeguard |
| serde | 1.0.228 | Apache-2.0 OR MIT | Erick Tryzelaar <erick.tryzelaar@gmail.com>, David Tolnay <d | https://github.com/serde-rs/serde |
| serde_core | 1.0.228 | Apache-2.0 OR MIT | Erick Tryzelaar <erick.tryzelaar@gmail.com>, David Tolnay <d | https://github.com/serde-rs/serde |
| serde_derive | 1.0.228 | Apache-2.0 OR MIT | Erick Tryzelaar <erick.tryzelaar@gmail.com>, David Tolnay <d | https://github.com/serde-rs/serde |
| sha2 | 0.10.9 | Apache-2.0 OR MIT | RustCrypto Developers | https://github.com/RustCrypto/hashes |
| shlex | 2.0.1 | Apache-2.0 OR MIT | comex <comexk@gmail.com>, Fenhl <fenhl@fenhl.net>, Adrian Ta | https://github.com/comex/rust-shlex |
| simd-adler32 | 0.3.9 | MIT | Marvin Countryman <me@maar.vin> | https://github.com/mcountryman/simd-adler32 |
| smallvec | 1.15.2 | Apache-2.0 OR MIT | The Servo Project Developers | https://github.com/servo/rust-smallvec |
| smol_str | 0.3.6 | Apache-2.0 OR MIT | Aleksey Kladov <aleksey.kladov@gmail.com>, Lukas Wirth <luka | https://github.com/rust-lang/rust-analyzer/tree/master/lib/smol_str |
| static_assertions | 1.1.0 | Apache-2.0 OR MIT | Nikolai Vazquez | https://github.com/nvzqz/static-assertions-rs |
| strength_reduce | 0.2.4 | Apache-2.0 OR MIT | Elliott Mahler <join.together@gmail.com> | http://github.com/ejmahler/strength_reduce |
| strict-num | 0.1.1 | MIT | Yevhenii Reizner <razrfalcon@gmail.com> | https://github.com/RazrFalcon/strict-num |
| string-interner | 0.15.0 | Apache-2.0 OR MIT | Robbepop | https://github.com/robbepop/string-interner |
| syn | 1.0.109 | Apache-2.0 OR MIT | David Tolnay <dtolnay@gmail.com> | https://github.com/dtolnay/syn |
| syn | 2.0.117 | Apache-2.0 OR MIT | David Tolnay <dtolnay@gmail.com> | https://github.com/dtolnay/syn |
| tar | 0.4.46 | Apache-2.0 OR MIT | Alex Crichton <alex@alexcrichton.com> | https://github.com/composefs/tar-rs |
| target-lexicon | 0.13.5 | Apache-2.0 WITH LLVM-exception | Dan Gohman <sunfish@mozilla.com> | https://github.com/bytecodealliance/target-lexicon |
| thiserror | 2.0.18 | Apache-2.0 OR MIT | David Tolnay <dtolnay@gmail.com> | https://github.com/dtolnay/thiserror |
| thiserror-impl | 2.0.18 | Apache-2.0 OR MIT | David Tolnay <dtolnay@gmail.com> | https://github.com/dtolnay/thiserror |
| tiff | 0.11.3 | MIT | The image-rs Developers | https://github.com/image-rs/image-tiff |
| time | 0.3.49 | Apache-2.0 OR MIT | Jacob Pratt <open-source@jhpratt.dev>, Time contributors | https://github.com/time-rs/time |
| time-core | 0.1.9 | Apache-2.0 OR MIT | Jacob Pratt <open-source@jhpratt.dev>, Time contributors | https://github.com/time-rs/time |
| time-macros | 0.2.29 | Apache-2.0 OR MIT | Jacob Pratt <open-source@jhpratt.dev>, Time contributors | https://github.com/time-rs/time |
| tiny-skia | 0.11.4 | BSD-3-Clause | Yevhenii Reizner <razrfalcon@gmail.com> | https://github.com/RazrFalcon/tiny-skia |
| tiny-skia-path | 0.11.4 | BSD-3-Clause | Yevhenii Reizner <razrfalcon@gmail.com> | https://github.com/RazrFalcon/tiny-skia/tree/master/path |
| tinyvec | 1.11.0 | Apache-2.0 OR MIT OR Zlib | Lokathor <zefria@gmail.com> | https://github.com/Lokathor/tinyvec |
| tinyvec_macros | 0.1.1 | Apache-2.0 OR MIT OR Zlib | Soveu <marx.tomasz@gmail.com> | https://github.com/Soveu/tinyvec_macros |
| tract-core | 0.21.10 | Apache-2.0 OR MIT | Mathieu Poumeyrol <kali@zoy.org> | https://github.com/snipsco/tract |
| tract-data | 0.21.10 | Apache-2.0 OR MIT | Mathieu Poumeyrol <kali@zoy.org> | https://github.com/snipsco/tract |
| tract-hir | 0.21.10 | Apache-2.0 OR MIT | Mathieu Poumeyrol <kali@zoy.org> | https://github.com/snipsco/tract |
| tract-linalg | 0.21.10 | Apache-2.0 OR MIT | Mathieu Poumeyrol <kali@zoy.org> | https://github.com/snipsco/tract |
| tract-nnef | 0.21.10 | Apache-2.0 OR MIT | Mathieu Poumeyrol <kali@zoy.org> | https://github.com/snipsco/tract |
| tract-onnx | 0.21.10 | Apache-2.0 OR MIT | Mathieu Poumeyrol <kali@zoy.org> | https://github.com/snipsco/tract |
| tract-onnx-opl | 0.21.10 | Apache-2.0 OR MIT | Mathieu Poumeyrol <kali@zoy.org> | https://github.com/snipsco/tract |
| transpose | 0.2.3 | Apache-2.0 OR MIT | Elliott Mahler <join.together@gmail.com> | https://github.com/ejmahler/transpose |
| ttf-parser | 0.25.1 | Apache-2.0 OR MIT | Caleb Maclennan <caleb@alerque.com>, Laurenz Stampfl <lauren | https://github.com/harfbuzz/ttf-parser |
| typenum | 1.20.1 | Apache-2.0 OR MIT |  | https://github.com/paholg/typenum |
| ucd-trie | 0.1.7 | Apache-2.0 OR MIT | Andrew Gallant <jamslam@gmail.com> | https://github.com/BurntSushi/ucd-generate |
| unicase | 2.9.0 | Apache-2.0 OR MIT | Sean McArthur <sean@seanmonstar.com> | https://github.com/seanmonstar/unicase |
| unicode-ident | 1.0.24 | (Apache-2.0 OR MIT) AND Unicode-3.0 | David Tolnay <dtolnay@gmail.com> | https://github.com/dtolnay/unicode-ident |
| unicode-normalization | 0.1.25 | Apache-2.0 OR MIT | kwantam <kwantam@gmail.com>, Manish Goregaokar <manishsmail@ | https://github.com/unicode-rs/unicode-normalization |
| unicode-segmentation | 1.13.3 | Apache-2.0 OR MIT | kwantam <kwantam@gmail.com>, Manish Goregaokar <manishsmail@ | https://github.com/unicode-rs/unicode-segmentation |
| version_check | 0.9.5 | Apache-2.0 OR MIT | Sergio Benitez <sb@sergio.bz> | https://github.com/SergioBenitez/version_check |
| walkdir | 2.5.0 | MIT OR Unlicense | Andrew Gallant <jamslam@gmail.com> | https://github.com/BurntSushi/walkdir |
| wasi | 0.11.1+wasi-snapshot-preview1 | Apache-2.0 OR Apache-2.0 WITH LLVM-exception OR MIT | The Cranelift Project Developers | https://github.com/bytecodealliance/wasi |
| weezl | 0.1.12 | Apache-2.0 OR MIT | The image-rs Developers | https://github.com/image-rs/weezl |
| weezl | 0.2.1 | Apache-2.0 OR MIT | The image-rs Developers | https://github.com/image-rs/weezl |
| winapi-util | 0.1.11 | MIT OR Unlicense | Andrew Gallant <jamslam@gmail.com> | https://github.com/BurntSushi/winapi-util |
| windows-link | 0.2.1 | Apache-2.0 OR MIT |  | https://github.com/microsoft/windows-rs |
| windows-sys | 0.61.2 | Apache-2.0 OR MIT |  | https://github.com/microsoft/windows-rs |
| xattr | 1.6.1 | Apache-2.0 OR MIT | Steven Allen <steven@stebalien.com> | https://github.com/Stebalien/xattr |
| zerocopy | 0.8.52 | Apache-2.0 OR BSD-2-Clause OR MIT | Joshua Liebow-Feeser <joshlf@google.com>, Jack Wrenn <jswren | https://github.com/google/zerocopy |
| zerocopy-derive | 0.8.52 | Apache-2.0 OR BSD-2-Clause OR MIT | Joshua Liebow-Feeser <joshlf@google.com>, Jack Wrenn <jswren | https://github.com/google/zerocopy |
| zune-core | 0.5.1 | Apache-2.0 OR MIT OR Zlib |  | https://github.com/etemesi254/zune-image |
| zune-jpeg | 0.5.15 | Apache-2.0 OR MIT OR Zlib | caleb <etemesicaleb@gmail.com> | https://github.com/etemesi254/zune-image/tree/dev/crates/zune-jpeg |

---

## 许可证全文 / 引用

### 0BSD

```
BSD Zero Clause License (0BSD)

Permission to use, copy, modify, and/or distribute this software for any purpose
with or without fee is hereby granted.

THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES WITH
REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF MERCHANTABILITY AND
FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY SPECIAL, DIRECT,
INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES WHATSOEVER RESULTING FROM LOSS
OF USE, DATA OR PROFITS, WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR OTHER
TORTIOUS ACTION, ARISING OUT OF OR IN CONNECTION WITH THE USE OR PERFORMANCE OF
THIS SOFTWARE.
```

### Apache-2.0

```
Apache License 2.0 — 全文见随分发物附带的 LICENSE 文件，或 https://www.apache.org/licenses/LICENSE-2.0
```

### BSD-2-Clause

```
BSD 2-Clause License

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are met:

1. Redistributions of source code must retain the above copyright notice, this
   list of conditions and the following disclaimer.
2. Redistributions in binary form must reproduce the above copyright notice,
   this list of conditions and the following disclaimer in the documentation
   and/or other materials provided with the distribution.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND
ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED
WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR
ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES
(INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES;
LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON
ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
(INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS
SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```

### BSD-3-Clause

```
BSD 3-Clause License

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are met:

1. Redistributions of source code must retain the above copyright notice, this
   list of conditions and the following disclaimer.
2. Redistributions in binary form must reproduce the above copyright notice,
   this list of conditions and the following disclaimer in the documentation
   and/or other materials provided with the distribution.
3. Neither the name of the copyright holder nor the names of its contributors
   may be used to endorse or promote products derived from this software without
   specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND
ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED
WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR
ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES
(INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES;
LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON
ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
(INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS
SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```

### BlueOak-1.0.0

```
Blue Oak Model License 1.0.0 — 全文见 https://blueoakcouncil.org/license/1.0.0
```

### LLVM-exception

```
Apache-2.0 WITH LLVM-exception — 全文见 https://spdx.org/licenses/LLVM-exception.html
```

### MIT

```
MIT License

Permission is hereby granted, free of charge, to any person obtaining a copy of
this software and associated documentation files (the "Software"), to deal in
the Software without restriction, including without limitation the rights to
use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of
the Software, and to permit persons to whom the Software is furnished to do so,
subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS
FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR
COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER
IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN
CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
```

### Unicode-3.0

```
Unicode License v3 — 全文见 https://spdx.org/licenses/Unicode-3.0.html
```

### Unlicense

```
The Unlicense

This is free and unencumbered software released into the public domain.

Anyone is free to copy, modify, publish, use, compile, sell, or distribute this
software, either in source code form or as a compiled binary, for any purpose,
commercial or non-commercial, and by any means.

In jurisdictions that recognize copyright laws, the author or authors of this
software dedicate any and all copyright interest in the software to the public
domain. We make this dedication for the benefit of the public at large and to
the detriment of our heirs and successors. We intend this dedication to be an
overt act of relinquishment in perpetuity of all present and future rights to
this software under copyright law.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS
FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS BE
LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF
CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE
SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

For more information, please refer to <https://unlicense.org/>
```

### Zlib

```
zlib License

This software is provided 'as-is', without any express or implied warranty. In
no event will the authors be held liable for any damages arising from the use of
this software.

Permission is granted to anyone to use this software for any purpose, including
commercial applications, and to alter it and redistribute it freely, subject to
the following restrictions:

1. The origin of this software must not be misrepresented; you must not claim
   that you wrote the original software. If you use this software in a product,
   an acknowledgment in the product documentation would be appreciated but is
   not required.
2. Altered source versions must be plainly marked as such, and must not be
   misrepresented as being the original software.
3. This notice may not be removed or altered from any source distribution.
```
