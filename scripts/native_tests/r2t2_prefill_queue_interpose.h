#pragma once

// Force-included only in the runtime TU. The queue executes real CPU graphs.
#include "engine/framework/core/backend.h"

void r2t2_queue_tensor_set(ggml_backend_t backend, ggml_tensor * tensor,
                         const void * data, size_t offset, size_t size);
void r2t2_queue_sync_tensor_set(ggml_tensor * tensor, const void * data,
                              size_t offset, size_t size);
void r2t2_queue_synchronize(ggml_backend_t backend);
ggml_status r2t2_queue_graph_compute_async(ggml_backend_t backend, ggml_cgraph * graph);

namespace engine::core {
ggml_status r2t2_queue_compute_backend_graph(
    ggml_backend_t backend, ggml_cgraph * graph,
    ggml_backend_graph_plan_t plan = nullptr, const char * label = nullptr);
}

#define ggml_backend_tensor_set_async r2t2_queue_tensor_set
#define ggml_backend_tensor_set r2t2_queue_sync_tensor_set
#define ggml_backend_synchronize r2t2_queue_synchronize
#define ggml_backend_graph_compute_async r2t2_queue_graph_compute_async
#define compute_backend_graph r2t2_queue_compute_backend_graph
