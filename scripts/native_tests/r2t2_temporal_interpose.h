#pragma once

#include "engine/framework/core/backend.h"
#include <ggml-alloc.h>

void temporal_set_async(ggml_backend_t, ggml_tensor *, const void *, size_t, size_t);
void temporal_set(ggml_tensor *, const void *, size_t, size_t);
void temporal_get(const ggml_tensor *, void *, size_t, size_t);
void temporal_sync(ggml_backend_t);
void temporal_buffer_free(ggml_backend_buffer_t);
bool temporal_alloc(ggml_gallocr_t, ggml_cgraph *);
namespace engine::core {
ggml_status temporal_compute(ggml_backend_t, ggml_cgraph *, ggml_backend_graph_plan_t = nullptr, const char * = nullptr);
void temporal_release(ggml_backend_t, ggml_cgraph *, bool = false);
void temporal_release(BackendType, ggml_backend_t, ggml_cgraph *, bool = false);
}
#define ggml_backend_tensor_set_async temporal_set_async
#define ggml_backend_tensor_set temporal_set
#define ggml_backend_tensor_get temporal_get
#define ggml_backend_synchronize temporal_sync
#define ggml_backend_buffer_free temporal_buffer_free
#define ggml_gallocr_alloc_graph temporal_alloc
#define compute_backend_graph temporal_compute
#define release_backend_graph_resources temporal_release
