QwenCausalDecodeStepResult QwenCausalDecodeRuntime::prefill_device_embeddings_into_cache(
    ggml_tensor * embeddings, int64_t steps, int64_t cache_steps, int64_t chunk_steps) {
    if (!embeddings || embeddings->type != GGML_TYPE_F32 || !ggml_is_contiguous(embeddings) ||
        steps <= 0 || embeddings->ne[1] != steps || embeddings->ne[2] != 1 || embeddings->ne[3] != 1) {
        throw std::runtime_error("Qwen device prefill has invalid embeddings");
    }
    std::vector<float> values(static_cast<size_t>(ggml_nelements(embeddings)));
    ggml_backend_tensor_get(embeddings, values.data(), 0, values.size() * sizeof(float));
    return impl_->prefill_embeddings_into_cache(values, steps, cache_steps, chunk_steps);
}

