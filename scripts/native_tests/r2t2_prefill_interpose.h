#pragma once



#include "engine/framework/core/backend.h"

namespace engine::core {
ggml_status r2t2_test_compute_backend_graph(
    ggml_backend_t backend, ggml_cgraph * graph,
    ggml_backend_graph_plan_t plan = nullptr, const char * label = nullptr);
}

#define compute_backend_graph r2t2_test_compute_backend_graph
