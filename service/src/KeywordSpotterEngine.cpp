#include "KeywordSpotterEngine.hpp"
#include <fstream>
#include <iostream>
#include <filesystem>
#include "sherpa-onnx/c-api/cxx-api.h"

namespace fs = std::filesystem;

namespace VoiceAssistant {

KeywordSpotterEngine::KeywordSpotterEngine() = default;

KeywordSpotterEngine::~KeywordSpotterEngine() {
    shutdown();
}

void KeywordSpotterEngine::log(const std::string& level, const std::string& message) {
    std::cout << "[KeywordSpotter] [" << level << "] " << message << std::endl;
}

bool KeywordSpotterEngine::initialize(const ModelPaths& paths, float threshold) {
    shutdown();
    m_threshold = threshold;
    m_paths = paths;
    m_hasPaths = true;

    const auto modelFiles = paths.findKwsModel();
    if (!modelFiles) {
        log("ERROR", "KWS model not found in " + paths.basePath());
        return false;
    }

    m_modelDir = fs::path(modelFiles->encoder).parent_path().string();

    sherpa_onnx::cxx::KeywordSpotterConfig config;
    config.feat_config.sample_rate = 16000;
    config.feat_config.feature_dim = 80;
    config.model_config.tokens = modelFiles->tokens;
    config.model_config.transducer.encoder = modelFiles->encoder;
    config.model_config.transducer.decoder = modelFiles->decoder;
    config.model_config.transducer.joiner = modelFiles->joiner;
    config.model_config.num_threads = 2;
    config.model_config.provider = "cpu";
    config.keywords_threshold = threshold;
    config.keywords_file = paths.kwsKeywordsPath();

    if (!fs::exists(config.keywords_file)) {
        writeKeywordsFile({"hey willow"});
    }

    try {
        m_spotter = std::make_unique<sherpa_onnx::cxx::KeywordSpotter>(
            sherpa_onnx::cxx::KeywordSpotter::Create(config));
        m_stream = std::make_unique<sherpa_onnx::cxx::OnlineStream>(m_spotter->CreateStream());
        m_loaded = true;
        log("INFO", "KWS engine initialized");
        return true;
    } catch (const std::exception& e) {
        log("ERROR", std::string("Failed to initialize KWS: ") + e.what());
        return false;
    }
}

void KeywordSpotterEngine::shutdown() {
    m_stream.reset();
    m_spotter.reset();
    m_loaded = false;
}

void KeywordSpotterEngine::writeKeywordsFile(const std::vector<std::string>& keywords) {
    const std::string path = m_modelDir.empty()
        ? std::string(std::getenv("HOME") ? std::getenv("HOME") : "/tmp") + "/.local/share/willow/models/kws/keywords.txt"
        : m_modelDir + "/keywords.txt";

    fs::create_directories(fs::path(path).parent_path());
    std::ofstream file(path);
    for (const auto& kw : keywords) {
        file << kw << "\n";
    }
    log("INFO", "Wrote keywords file: " + path);
}

void KeywordSpotterEngine::setKeywords(const std::vector<std::string>& keywords) {
    if (!m_hasPaths) return;
    writeKeywordsFile(keywords);
    initialize(m_paths, m_threshold);
}

void KeywordSpotterEngine::setThreshold(float threshold) {
    if (!m_hasPaths || threshold == m_threshold) return;
    m_threshold = threshold;
    initialize(m_paths, m_threshold);
}

void KeywordSpotterEngine::setCallback(KeywordCallback callback) {
    std::lock_guard<std::mutex> lock(m_mutex);
    m_callback = std::move(callback);
}

void KeywordSpotterEngine::processAudio(const std::vector<float>& chunk) {
    if (!m_loaded || !m_enabled || !m_spotter || !m_stream || chunk.empty()) return;

    std::lock_guard<std::mutex> lock(m_mutex);

    m_stream->AcceptWaveform(16000, chunk.data(), static_cast<int32_t>(chunk.size()));

    while (m_spotter->IsReady(m_stream.get())) {
        m_spotter->Decode(m_stream.get());
        auto result = m_spotter->GetResult(m_stream.get());
        if (!result.keyword.empty()) {
            log("INFO", "Keyword detected: " + result.keyword);
            if (m_callback) {
                m_callback(result.keyword);
            }
            m_spotter->Reset(m_stream.get());
        }
    }
}

} // namespace VoiceAssistant
