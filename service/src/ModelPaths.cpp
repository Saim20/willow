#include "ModelPaths.hpp"
#include <filesystem>
#include <algorithm>

namespace fs = std::filesystem;

namespace VoiceAssistant {

ModelPaths::ModelPaths(std::string basePath)
    : m_basePath(std::move(basePath)) {}

bool ModelPaths::fileExists(const std::string& path) {
    return fs::exists(path) && fs::is_regular_file(path);
}

std::vector<std::string> ModelPaths::listFiles(const std::string& dir, const std::string& suffix) {
    std::vector<std::string> result;
    if (!fs::exists(dir)) return result;
    for (const auto& entry : fs::directory_iterator(dir)) {
        if (!entry.is_regular_file()) continue;
        const auto name = entry.path().filename().string();
        if (name.size() >= suffix.size() &&
            name.compare(name.size() - suffix.size(), suffix.size(), suffix) == 0) {
            result.push_back(entry.path().string());
        }
    }
    return result;
}

std::optional<std::string> ModelPaths::findFirstExistingDir(const std::vector<std::string>& candidates) const {
    for (const auto& candidate : candidates) {
        if (fs::exists(candidate) && fs::is_directory(candidate)) {
            return candidate;
        }
    }
    return std::nullopt;
}

std::optional<TransducerModelFiles> ModelPaths::findTransducerInDir(const std::string& dir) const {
    if (!fs::exists(dir)) return std::nullopt;

    TransducerModelFiles files;
    files.tokens = dir + "/tokens.txt";
    if (!fileExists(files.tokens)) return std::nullopt;

    auto encoders = listFiles(dir, ".onnx");
    std::string encoderPath;
    std::string decoderPath;
    std::string joinerPath;

    for (const auto& path : encoders) {
        const auto filename = fs::path(path).filename().string();
        if (filename.find("encoder") != std::string::npos) {
            encoderPath = path;
        } else if (filename.find("decoder") != std::string::npos) {
            decoderPath = path;
        } else if (filename.find("joiner") != std::string::npos) {
            joinerPath = path;
        }
    }

    if (encoderPath.empty() || decoderPath.empty() || joinerPath.empty()) {
        return std::nullopt;
    }

    files.encoder = encoderPath;
    files.decoder = decoderPath;
    files.joiner = joinerPath;
    return files;
}

std::optional<TransducerModelFiles> ModelPaths::findKwsModel() const {
    const auto dir = findFirstExistingDir({
        m_basePath + "/kws",
        m_basePath + "/kws-zipformer-en",
    });
    if (!dir) return std::nullopt;
    return findTransducerInDir(*dir);
}

std::optional<TransducerModelFiles> ModelPaths::findStreamingModel() const {
    const auto dir = findFirstExistingDir({
        m_basePath + "/streaming",
        m_basePath + "/streaming-zipformer-en",
    });
    if (!dir) return std::nullopt;
    return findTransducerInDir(*dir);
}

std::optional<std::string> ModelPaths::findSpeakerModel() const {
    const auto dir = findFirstExistingDir({
        m_basePath + "/speaker",
        m_basePath + "/speaker-resemblyzer",
        m_basePath + "/wespeaker",
    });
    if (!dir) return std::nullopt;

    for (const auto& entry : fs::directory_iterator(*dir)) {
        if (!entry.is_regular_file()) continue;
        const auto name = entry.path().filename().string();
        if (name.find("model") != std::string::npos && name.ends_with(".onnx")) {
            return entry.path().string();
        }
    }

    const auto onnxFiles = listFiles(*dir, ".onnx");
    if (!onnxFiles.empty()) {
        return onnxFiles.front();
    }
    return std::nullopt;
}

std::optional<std::string> ModelPaths::findTtsModelDir() const {
    return findFirstExistingDir({
        m_basePath + "/tts",
        m_basePath + "/piper-en",
    });
}

std::string ModelPaths::kwsKeywordsPath() const {
    const std::vector<std::string> candidates = {
        m_basePath + "/kws/keywords.txt",
        m_basePath + "/keywords.txt",
    };
    for (const auto& path : candidates) {
        if (fileExists(path)) return path;
    }
    return m_basePath + "/kws/keywords.txt";
}

std::string ModelPaths::speakerProfilePath() const {
    const char* home = std::getenv("HOME");
    if (home) {
        return std::string(home) + "/.config/willow/speaker_profile.bin";
    }
    return "/tmp/willow_speaker_profile.bin";
}

} // namespace VoiceAssistant
