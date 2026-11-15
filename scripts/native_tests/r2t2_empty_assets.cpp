#include "engine/framework/assets/resource_bundle.h"

#include <utility>



namespace engine::assets {
ResourceBundle::ResourceBundle(std::filesystem::path model_root)
    : model_root_(std::move(model_root)) {}
}
