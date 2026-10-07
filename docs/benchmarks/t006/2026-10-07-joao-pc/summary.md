# T006 benchmark - AMD Ryzen 5 5600H with Radeon Graphics

Microsoft Windows 11 Pro 10.0.26300 (build 26300) | 15.3 GB RAM | GPUs: NVIDIA GeForce GTX 1650, AMD Radeon(TM) Graphics | AC power: True | onnxruntime 1.30.0 (CPU) / onnxruntime-directml 1.24.4 (DirectML)

Query = single short search query (warm). 128-tok = one ~100-word input. docs/s = 200-word chunks at batch 8. GPU nodes % = graph nodes on DirectML (rest on CPU EP).

| run | query p50 ms | p95 | ~128-tok p50 | docs/s (b8) | cold load ms | RSS warm MiB | NVIDIA VRAM +MiB | GPU nodes % | min cos | status |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|
| cpu-fp32 | 33.1 | 38.0 | 129.6 | 3.2 | 1933 | 627 |  |  | 1.0000 | ok |
| cpu-q8 | 220.8 | 226.0 | 339.0 | 2.9 | 1300 | 104 |  |  | 0.9999 | ok |
| cpu-q4 | 30.0 | 36.9 | 133.2 | 3.1 | 1325 | 168 |  |  | 0.9797 | ok |
| cpu-q4-t4 | 45.5 | 49.4 | 190.3 | 2.5 | 1320 | 168 |  |  | 0.9797 | ok |
| cpu-fp32-t4 | 31.9 | 40.3 | 115.9 | 3.4 | 1736 | 624 |  |  | 1.0000 | ok |
| dml-high-fp16 | | | | | | | | | | FAILED: error: vector 0 has zero norm |
| dml-high-fp32 | 352.9 | 376.4 | 385.6 | 8.4 | 5445 | 164 | 2854 | 94.5 | 1.0000 | ok |
| dml-high-q4f16 | 571.7 | 600.0 | 575.8 | 4.6 | 3234 | 185 | 1089 | 99.0 | 0.9799 | ok |
| dml-high-q4 | 406.4 | 533.5 | 493.8 | 6.4 | 2891 | 180 | 2472 | 94.4 | 0.9797 | ok |
| dml-low-fp16 | | | | | | | | | | FAILED: GPU device hung/removed (DXGI 887A0006) during inference |
| dml-low-q4f16 | | | | | | | | | | FAILED: error: vector 0 has zero norm |
| dml-0-fp16 | 293.1 | 374.8 | 332.1 | 4.6 | 3693 | 169 | 1268 | 99.1 | 1.0000 | ok |
| dml-1-fp16 | | | | | | | | | | FAILED: GPU device hung/removed (DXGI 887A0006) during inference |
