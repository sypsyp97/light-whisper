#pragma once


#include "engine/framework/core/backend.h"

void r2t2_deferred_tensor_set(ggml_backend_t backend, ggml_tensor * tensor,
                            const void * data, size_t offset, size_t size);
void r2t2_observed_tensor_set(ggml_tensor * tensor, const void * data,
                            size_t offset, size_t size);
void r2t2_observed_synchronize(ggml_backend_t backend);
namespace engine::core {
ggml_status r2t2_deferred_compute_backend_graph(
    ggml_backend_t backend, ggml_cgraph * graph,
    ggml_backend_graph_plan_t plan = nullptr, const char * label = nullptr);
}

#define ggml_backend_tensor_set_async r2t2_deferred_tensor_set
#define ggml_backend_tensor_set r2t2_observed_tensor_set
#define ggml_backend_synchronize r2t2_observed_synchronize
#define compute_backend_graph r2t2_deferred_compute_backend_graph
