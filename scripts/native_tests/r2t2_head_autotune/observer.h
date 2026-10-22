#pragma once
#include "ggml-cuda/common.cuh"

// Declarations only. Implementations call the real stock kernel/runtime APIs.
void head_test_mmvf(ggml_backend_cuda_context &, const ggml_tensor *, const ggml_tensor *,
                    const ggml_tensor *, ggml_tensor *, const ggml_cuda_mm_fusion_args_host *);
cudaError_t head_test_event_record(cudaEvent_t, cudaStream_t = nullptr);
cudaError_t head_test_elapsed(float *, cudaEvent_t, cudaEvent_t);
cudaError_t head_test_event_create(cudaEvent_t *);
cudaError_t head_test_event_create_flags(cudaEvent_t *, unsigned int);
cudaError_t head_test_event_sync(cudaEvent_t);
cudaError_t head_test_stream_sync(cudaStream_t);
cudaError_t head_test_device_sync();
cudaError_t head_test_malloc(void **, size_t);
cudaError_t head_test_malloc_managed(void **, size_t, unsigned int = cudaMemAttachGlobal);
cudaError_t head_test_malloc_host(void **, size_t);
cudaError_t head_test_host_alloc(void **, size_t, unsigned int);
CUresult head_test_mem_create(CUmemGenericAllocationHandle *, size_t, const CUmemAllocationProp *, unsigned long long);
CUresult head_test_address_reserve(CUdeviceptr *, size_t, size_t, CUdeviceptr, unsigned long long);
