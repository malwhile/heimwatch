Figure out a way to gather GPU metrics, this one will be tricky so take your time to think and research possible ways to get the metrics. We are focused on Linux for now, so ignore Windows an MacOS. A few things to consider:

1. Is it possible to leverage ebpf and kernel space to get the stats
2. There could be multiple GPUs available from different vendors
    - On laptops this is usually an integrated GPU inside the processor and a dedicated GPU
    - The same could be true for desktops
    - Desktops could also contain multiple dedicated GPUs
3. Is there a way to gather stats generically, or do they need to come from APIs per specific driver
4. If we need to gather per API, is there a way to detect the hardware and drivers and dynamically load the proper code
    - We should use a linux directory with a separate rust file per gpu type
5. Do some of the APIs overlap, aka see how much code can be shared

Create a document docs/gpu-metrics.md with the decision making for how to get GPU stats
