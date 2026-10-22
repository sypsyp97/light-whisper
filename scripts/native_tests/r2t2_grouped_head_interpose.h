#pragma once
// Force-include only in the isolated runtime owner translation unit.
#include "engine/framework/core/backend.h"
#include <ggml-alloc.h>
bool grouped_head_alloc(ggml_gallocr_t, ggml_cgraph *);
namespace engine::core {
ggml_status grouped_head_compute(ggml_backend_t, ggml_cgraph *, ggml_backend_graph_plan_t = nullptr, const char * = nullptr);
}
#define compute_backend_graph grouped_head_compute
#define ggml_gallocr_alloc_graph grouped_head_alloc
