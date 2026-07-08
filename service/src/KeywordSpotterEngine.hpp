#pragma once

#include "ModelPaths.hpp"
#include "sherpa-onnx/c-api/cxx-api.h"
#include <string>
#include <vector>
#include <functional>
#include <memory>
#include <mutex>

namespace VoiceAssistant {

class KeywordSpotterEngine {
public:
    using KeywordCallback = std::function<void(const std::string& keyword)>;

    KeywordSpotterEngine();
    ~KeywordSpotterEngine();

    bool initialize(const ModelPaths& paths, float threshold = 0.25f);
    void shutdown();
    bool isLoaded() const { return m_loaded; }

    void setKeywords(const std::vector<std::string>& keywords);
    void setThreshold(float threshold);
    void setCallback(KeywordCallback callback);
    void processAudio(const std::vector<float>& chunk);

    void setEnabled(bool enabled) { m_enabled = enabled; }
    bool isEnabled() const { return m_enabled; }

private:
    std::unique_ptr<sherpa_onnx::cxx::KeywordSpotter> m_spotter;
    std::unique_ptr<sherpa_onnx::cxx::OnlineStream> m_stream;
    KeywordCallback m_callback;
    std::mutex m_mutex;
    bool m_loaded{false};
    bool m_enabled{true};
    float m_threshold{0.25f};
    std::string m_modelDir;
    ModelPaths m_paths{""};
    bool m_hasPaths{false};

    void writeKeywordsFile(const std::vector<std::string>& keywords);
    void log(const std::string& level, const std::string& message);
};

} // namespace VoiceAssistant
