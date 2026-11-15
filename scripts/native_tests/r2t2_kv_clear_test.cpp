
#include "engine/framework/runtime/kv_cache.h"
#include "ggml-backend-impl.h"
#include "ggml-cpu.h"

#include <algorithm>
#include <cstdint>
#include <cstring>
#include <fstream>
#include <iostream>
#include <stdexcept>
#include <string>
#include <vector>

namespace {
namespace core = engine::core;
namespace runtime = engine::runtime;
using Bytes = std::vector<std::vector<unsigned char>>;
size_t assertions = 0, cases = 0, rejected = 0, injected = 0, work_failures = 0;
size_t total_hooks = 0, total_memsets = 0, total_syncs = 0, total_consumed = 0, total_graphs = 0;

void require(bool condition, const char * message) {
    ++assertions;
    if (!condition) { throw std::runtime_error(message); }
}
void work_check(bool condition, const char * message) {
    if (!condition) { ++work_failures; std::cout << "WORK_RED " << message << '\n'; }
}
template<class T> void record(std::ofstream & trace, const T & value) {
    trace.write(reinterpret_cast<const char *>(&value), sizeof(value));
}


void clear(runtime::TransformerKVCache & cache, ggml_backend_t backend, int64_t keep) {
#ifdef BASELINE
    (void)backend;
    cache.clear_on_backend(keep);
#else
    cache.clear_on_backend(backend, keep);
#endif
}

struct Fixture {
    struct Pending { ggml_tensor * tensor; uint8_t value; size_t offset, bytes; };
    const int64_t capacity = 7, step_elems = 6;
    ggml_type type;
    bool hook_enabled, active = false;
    int64_t keep = 0, old_valid = 0, old_end = 0;
    size_t hooks = 0, memsets = 0, syncs = 0, consumed = 0, graphs = 0, throw_on_hook = 0;
    ggml_context * context = nullptr;
    ggml_backend_t backend = nullptr;
    ggml_backend_buffer_t allocation = nullptr;
    ggml_backend_dev_t original_device = nullptr;
    ggml_backend_reg_t original_registry = nullptr;
    ggml_backend_device device = {};
    ggml_backend_reg registry = {};
    ggml_backend_buffer observed_buffer = {};
    void (*real_sync)(ggml_backend_t) = nullptr;
    ggml_status (*real_compute)(ggml_backend_t, ggml_cgraph *) = nullptr;
    void (*real_memset)(ggml_backend_buffer_t, ggml_tensor *, uint8_t, size_t, size_t) = nullptr;
    std::vector<ggml_tensor *> tensors;
    std::vector<Pending> pending;
    Bytes before;
    runtime::TransformerKVCache cache;

    Fixture(ggml_type type_, bool hook = true, bool ring = false, int layers = 3)
        : type(type_), hook_enabled(hook) {
        context = ggml_init({1024 * 1024, nullptr, true});
        backend = ggml_backend_cpu_init();
        require(context && backend, "CPU fixture allocation failed");
        ggml_backend_cpu_set_n_threads(backend, 1);
        std::vector<core::TensorValue> keys, values;
        for (int layer = 0; layer < layers; ++layer) {
            auto make = [&](int shape) {
                auto * tensor = shape == 0 ? ggml_new_tensor_1d(context, type, capacity * step_elems)
                    : shape == 1 ? ggml_new_tensor_2d(context, type, step_elems, capacity)
                    : ggml_new_tensor_3d(context, type, step_elems / 2, 2, capacity);
                tensors.push_back(tensor);
                const auto logical = shape == 0 ? core::TensorShape::from_dims({capacity * step_elems})
                    : shape == 1 ? core::TensorShape::from_dims({capacity, step_elems})
                    : core::TensorShape::from_dims({capacity, 2, step_elems / 2});
                return core::TensorValue{tensor, logical, type};
            };
            keys.push_back(make(layer % 3));
            values.push_back(make((layer + 1) % 3));
        }
        allocation = ggml_backend_alloc_ctx_tensors(context, backend);
        require(tensors.empty() || allocation, "CPU tensor allocation failed");
        runtime::TransformerKVCacheOptions options;
        options.allow_f16_storage = type == GGML_TYPE_F16;
        options.allow_bf16_storage = type == GGML_TYPE_BF16;
        options.ring_mode = ring;
        options.ring_pinned_steps = ring ? 1 : 0;
        cache = runtime::TransformerKVCache(capacity, step_elems, keys, values, options);
        runtime::TransformerKVState state;
        state.current_end = capacity + 17;
        state.layers.resize(keys.size());
        for (auto & layer : state.layers) {
            layer.valid_steps = capacity;
            layer.key.assign(static_cast<size_t>(capacity * step_elems), 1.0F);
            layer.value = layer.key;
        }
        cache.import_state(state);


        original_device = backend->device;
        original_registry = original_device->reg;
        device = *original_device;
        registry = *original_registry;
        registry.context = this;
        registry.iface.get_proc_address = get_proc;
        device.reg = &registry;
        backend->device = &device;
        real_sync = backend->iface.synchronize;
        real_compute = backend->iface.graph_compute;
        backend->iface.synchronize = synchronize;
        backend->iface.graph_compute = compute;
        if (allocation) {


            observed_buffer = *allocation;
            observed_buffer.context = this;
            real_memset = allocation->iface.memset_tensor;
            observed_buffer.iface.memset_tensor = memset_sync;
            for (auto * tensor : tensors) { tensor->buffer = &observed_buffer; }
        }
    }

    ~Fixture() {

        if (!pending.empty()) { std::cerr << "FAIL pending work survived clear boundary\n"; std::terminate(); }
        for (auto * tensor : tensors) { tensor->buffer = allocation; }
        backend->device = original_device;
        ggml_backend_buffer_free(allocation);
        ggml_backend_free(backend);
        ggml_free(context);
    }

    static Fixture & owner(ggml_backend_t backend) {
        return *static_cast<Fixture *>(backend->device->reg->context);
    }
    static void * get_proc(ggml_backend_reg_t reg, const char * name) {
        auto & f = *static_cast<Fixture *>(reg->context);
        if (std::strcmp(name, "ggml_backend_cuda_memset_tensor_async") == 0) {
            return f.hook_enabled ? reinterpret_cast<void *>(memset_async) : nullptr;
        }
        return ggml_backend_reg_get_proc_address(f.original_registry, name);
    }
    void range(ggml_tensor * tensor, uint8_t value, size_t offset, size_t count) const {
        require(std::find(tensors.begin(), tensors.end(), tensor) != tensors.end(), "clear targeted foreign tensor");
        require(tensor->type == type && value == 0, "clear changed storage precision or zero value");
        require(offset == ggml_row_size(type, keep * step_elems) &&
                count == ggml_nbytes(tensor) - offset && count != 0, "clear used wrong suffix byte range");
    }
    void old_metadata() const {
        require(cache.valid_steps() == old_valid && cache.current_end() == old_end,
                "metadata advanced before queued work completed");
    }
    static void memset_async(ggml_backend_t backend, ggml_tensor * tensor,
                             uint8_t value, size_t offset, size_t count) {
        auto & f = owner(backend);
        require(f.backend == backend && f.active && f.hook_enabled, "hook used wrong backend or lifetime");
        f.range(tensor, value, offset, count);
        f.old_metadata();
        require(f.bytes() == f.before, "async hook executed writes before synchronization");
        require(std::none_of(f.pending.begin(), f.pending.end(),
                            [&](const Pending & p) { return p.tensor == tensor; }), "tensor queued twice");
        ++f.hooks; ++total_hooks;
        if (f.throw_on_hook && f.hooks == f.throw_on_hook) {
            throw std::runtime_error("injected async memset failure");
        }
        f.pending.push_back({tensor, value, offset, count});
    }
    static void synchronize(ggml_backend_t backend) {
        auto & f = owner(backend);
        if (f.active) { ++f.syncs; ++total_syncs; }
        if (!f.pending.empty()) {
            f.old_metadata();
            require(f.bytes() == f.before, "queued bytes changed before drain");
            for (const auto & p : f.pending) {

                f.real_memset(f.allocation, p.tensor, p.value, p.offset, p.bytes);
                ++f.consumed; ++total_consumed;
            }
            f.pending.clear();
        }
        if (f.real_sync) { f.real_sync(backend); }
    }
    static void memset_sync(ggml_backend_buffer_t buffer, ggml_tensor * tensor,
                            uint8_t value, size_t offset, size_t count) {
        auto & f = *static_cast<Fixture *>(buffer->context);
        if (f.active) { f.range(tensor, value, offset, count); ++f.memsets; ++total_memsets; }
        f.real_memset(f.allocation, tensor, value, offset, count);
    }
    static ggml_status compute(ggml_backend_t backend, ggml_cgraph * graph) {
        auto & f = owner(backend);
        if (f.active) { ++f.graphs; ++total_graphs; }
        return f.real_compute(backend, graph);
    }
    Bytes bytes() const {
        Bytes result;
        for (auto * tensor : tensors) {
            result.emplace_back(ggml_nbytes(tensor));
            ggml_backend_tensor_get(tensor, result.back().data(), 0, result.back().size());
        }
        return result;
    }
    void poison() {
        active = false;
        const uint32_t f32[] = {0x7fc12345, 0xff800000, 0x80000000, 0x3f8ccccd, 0x7f800000, 0xffc54321, 0xbf19999a};
        const uint16_t f16[] = {0x7e55, 0xfc00, 0x8000, 0x3c01, 0x7c00, 0xfe33, 0xb955};
        const uint16_t bf16[] = {0x7fc1, 0xff80, 0x8000, 0x3f81, 0x7f80, 0xffe5, 0xbf19};
        for (size_t t = 0; t < tensors.size(); ++t) {
            std::vector<unsigned char> raw(ggml_nbytes(tensors[t]));
            const size_t width = ggml_type_size(type);
            for (size_t i = 0; i < raw.size() / width; ++i) {
                const size_t index = (i + t) % 7;
                const void * word = type == GGML_TYPE_F32 ? static_cast<const void *>(&f32[index])
                    : type == GGML_TYPE_F16 ? static_cast<const void *>(&f16[index])
                    : static_cast<const void *>(&bf16[index]);
                std::memcpy(raw.data() + i * width, word, width);
            }
            ggml_backend_tensor_set(tensors[t], raw.data(), 0, raw.size());
        }
    }
    void observe(int64_t keep_, size_t throw_on = 0) {
        require(pending.empty(), "previous clear left queued writes");
        hooks = memsets = syncs = consumed = graphs = 0;
        keep = keep_; throw_on_hook = throw_on;
        old_valid = cache.valid_steps(); old_end = cache.current_end();
        before = bytes(); active = true;
    }
};

void check_bytes(Fixture & f) {
    const auto after = f.bytes();
    const size_t offset = ggml_row_size(f.type, f.keep * f.step_elems);
    for (size_t i = 0; i < after.size(); ++i) {
        require(std::equal(f.before[i].begin(), f.before[i].begin() + offset, after[i].begin()),
                "kept prefix changed bits (NaN payload/Inf/negative zero)");
        require(std::all_of(after[i].begin() + offset, after[i].end(),
                            [](unsigned char value) { return value == 0; }), "cleared suffix retained nonzero bits");
    }
}
void successful_case(Fixture & f, int64_t keep, std::ofstream & trace, bool null_backend = false) {
    f.poison(); f.observe(keep);
    clear(f.cache, null_backend ? nullptr : f.backend, keep);
    f.active = false;
    require(f.cache.valid_steps() == keep && f.cache.current_end() == keep, "clear metadata differs from kept prefix");
    require(f.pending.empty(), "successful clear returned with pending writes");
    check_bytes(f);
    const auto after = f.bytes();
    record(trace, f.capacity); record(trace, f.type); record(trace, keep);
    record(trace, f.cache.valid_steps()); record(trace, f.cache.current_end());
    for (const auto & raw : after) { trace.write(reinterpret_cast<const char *>(raw.data()), raw.size()); }
    const size_t count = keep == f.capacity ? 0 : f.tensors.size();
    if (count && f.hook_enabled && !null_backend) {
        work_check(f.hooks == count && f.syncs == 1 && f.memsets == 0 && f.consumed == count && f.graphs == 0,
                   "queued suffix clear requires every tensor hook, one drain, zero native sync memsets/graphs");
    } else {
        work_check(f.hooks == 0 && f.syncs == 0 && f.memsets == count && f.consumed == 0 && f.graphs == 0,
                   "fallback/no-op clear performed unexpected queued or graph work");
    }
    const auto consumed = f.consumed;
    ggml_backend_synchronize(f.backend);
    require(f.consumed == consumed && f.bytes() == after, "second synchronization consumed queued writes twice");
    ++cases;
}
void reject_case(Fixture & f, int64_t keep) {
    f.poison(); f.observe(keep);
    bool threw = false;
    try { clear(f.cache, f.backend, keep); } catch (const std::runtime_error &) { threw = true; }
    f.active = false;
    require(threw, "invalid prefix/ring preservation accepted");
    f.old_metadata();
    require(f.bytes() == f.before && f.pending.empty(), "rejected clear modified tensor bytes or queued work");
    require(f.hooks == 0 && f.syncs == 0 && f.memsets == 0 && f.graphs == 0, "rejected clear performed backend work");
    ++rejected;
}
void successful_suite(std::ofstream & trace) {
    for (const auto type : {GGML_TYPE_F32, GGML_TYPE_F16, GGML_TYPE_BF16}) {
        for (bool hook : {false, true}) {
            for (int64_t keep : {0, 3, 7}) {
                Fixture f(type, hook);
                successful_case(f, keep, trace);
            }
        }
    }
    { Fixture f(GGML_TYPE_F32); successful_case(f, 2, trace, true); }
    { Fixture f(GGML_TYPE_F32, true, true); reject_case(f, 1); successful_case(f, 0, trace); }
    { Fixture f(GGML_TYPE_F32); reject_case(f, -1); reject_case(f, 8); f.cache.retain_prefix(2); reject_case(f, 3); }
    { Fixture f(GGML_TYPE_F32, true, false, 0); successful_case(f, 0, trace); }
    { Fixture f(GGML_TYPE_BF16); successful_case(f, 3, trace); successful_case(f, 0, trace); }
}
void failure_suite() {
#ifndef BASELINE
    Fixture f(GGML_TYPE_F32);
    f.poison(); f.observe(3, 3);
    bool injected_error = false;
    try { clear(f.cache, f.backend, 3); }
    catch (const std::runtime_error & error) { injected_error = std::string(error.what()) == "injected async memset failure"; }
    f.active = false;
    require(injected_error && f.hooks == 3, "injected queued callback error did not propagate");
    require(f.syncs == 1 && f.pending.empty() && f.consumed == 2 && f.memsets == 0 && f.graphs == 0,
            "callback exception failed to drain borrowed tensor work before propagation");
    f.old_metadata();
    const auto after_failure = f.bytes();
    const size_t offset = ggml_row_size(f.type, f.keep * f.step_elems);
    for (size_t i = 0; i < after_failure.size(); ++i) {
        require(std::equal(f.before[i].begin(), f.before[i].begin() + offset, after_failure[i].begin()),
                "callback failure changed prefix bits");
        if (i < 2) {
            require(std::all_of(after_failure[i].begin() + offset, after_failure[i].end(),
                                [](unsigned char value) { return value == 0; }), "queued failure work did not complete");
        } else {
            require(after_failure[i] == f.before[i], "callback failure modified unqueued tensor");
        }
    }
    f.observe(3);
    clear(f.cache, f.backend, 3);
    f.active = false;
    require(f.hooks == f.tensors.size() && f.syncs == 1 && f.consumed == f.tensors.size() && f.pending.empty(),
            "clear did not recover after callback exception");
    require(f.cache.valid_steps() == 3 && f.cache.current_end() == 3, "recovery did not advance metadata");
    check_bytes(f);
    ++injected;
#endif
}
}

int main(int argc, char ** argv) {
    try {
        require(argc == 3, "usage: r2t2_kv_clear_{baseline,candidate}.exe equivalence|work trace.bin");
        const bool work = std::string(argv[1]) == "work";
        require(work || std::string(argv[1]) == "equivalence", "unknown mode");
        std::ofstream trace(argv[2], std::ios::binary);
        require(trace.good(), "trace open failed");
        successful_suite(trace);
        trace.close(); require(trace.good(), "trace write failed");
        failure_suite();
        std::cout << "KV_ASYNC_QUALITY_OK cases=" << cases << " rejected=" << rejected
                  << " injected_failures=" << injected << " assertions=" << assertions << '\n';
        std::cout << "KV_ASYNC_WORK hooks=" << total_hooks << " memsets=" << total_memsets
                  << " synchronizations=" << total_syncs << " consumed=" << total_consumed
                  << " graphs=" << total_graphs << " work_failures=" << work_failures << '\n';
        if (work && work_failures) {
            std::cerr << "FAIL queued suffix clear requires async hooks and one synchronization\n";
            return 1;
        }
        return 0;
    } catch (const std::exception & error) {
        std::cerr << "FAIL " << error.what() << '\n';
        return 1;
    }
}
