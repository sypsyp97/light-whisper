#pragma once

// Forced into the runtime TU only. The CPU backend and graph arithmetic stay real.
#include "engine/framework/core/backend.h"

void device_test_set(ggml_tensor *, const void *, size_t, size_t);
void device_test_set_async(ggml_backend_t, ggml_tensor *, const void *, size_t, size_t);
void device_test_get(const ggml_tensor *, void *, size_t, size_t);
void device_test_get_async(ggml_backend_t, const ggml_tensor *, void *, size_t, size_t);
void device_test_copy(const ggml_tensor *, ggml_tensor *);
void device_test_copy_async(ggml_backend_t, ggml_backend_t, const ggml_tensor *, ggml_tensor *);
namespace engine::core {
std::vector<float> device_test_read_f32(const ggml_tensor *);
void device_test_read_f32_into(const ggml_tensor *, std::vector<float> &);
ggml_status device_test_compute(ggml_backend_t, ggml_cgraph *,
                               ggml_backend_graph_plan_t = nullptr, const char * = nullptr);
}

#define ggml_backend_tensor_set device_test_set
#define ggml_backend_tensor_set_async device_test_set_async
#define ggml_backend_tensor_get device_test_get
#define ggml_backend_tensor_get_async device_test_get_async
#define ggml_backend_tensor_copy device_test_copy
#define ggml_backend_tensor_copy_async device_test_copy_async
#define read_tensor_f32 device_test_read_f32
#define read_tensor_f32_into device_test_read_f32_into
#define compute_backend_graph device_test_compute
