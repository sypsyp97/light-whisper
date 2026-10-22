#pragma once
// Force-include in ggml-cuda.cu only, after SDK/common declarations have loaded.
#include "observer.h"
#define ggml_cuda_mul_mat_vec_f head_test_mmvf
#define cudaEventRecord head_test_event_record
#define cudaEventElapsedTime head_test_elapsed
#define cudaEventCreate head_test_event_create
#define cudaEventCreateWithFlags head_test_event_create_flags
#define cudaEventSynchronize head_test_event_sync
#define cudaStreamSynchronize head_test_stream_sync
#define cudaDeviceSynchronize head_test_device_sync
#define cudaMalloc head_test_malloc
#define cudaMallocManaged head_test_malloc_managed
#define cudaMallocHost head_test_malloc_host
#define cudaHostAlloc head_test_host_alloc
#define cuMemCreate head_test_mem_create
#define cuMemAddressReserve head_test_address_reserve
