#pragma once

#include <string>
#include <vector>
#include <optional>

namespace VoiceAssistant {

struct TransducerModelFiles {
    std::string tokens;
    std::string encoder;
    std::string decoder;
    std::string joiner;
};

class ModelPaths {
public:
    explicit ModelPaths(std::string basePath);

    const std::string& basePath() const { return m_basePath; }

    std::optional<TransducerModelFiles> findKwsModel() const;
    std::optional<TransducerModelFiles> findStreamingModel() const;
    std::optional<std::string> findSpeakerModel() const;
    std::optional<std::string> findTtsModelDir() const;

    std::string kwsKeywordsPath() const;
    std::string speakerProfilePath() const;

    static bool fileExists(const std::string& path);
    static std::vector<std::string> listFiles(const std::string& dir, const std::string& suffix);

private:
    std::string m_basePath;

    std::optional<TransducerModelFiles> findTransducerInDir(const std::string& dir) const;
    std::optional<std::string> findFirstExistingDir(const std::vector<std::string>& candidates) const;
};

} // namespace VoiceAssistant
