#pragma once

#include "AudioRouter.hpp"
#include "KeywordSpotterEngine.hpp"
#include "StreamingASREngine.hpp"
#include "SpeakerVerifier.hpp"
#include "CommandIntentResolver.hpp"
#include "TypingStreamWriter.hpp"
#include "TtsEngine.hpp"
#include "ModelPaths.hpp"
#include "CommandExecutor.hpp"
#include "Types.hpp"
#include <string>
#include <vector>
#include <functional>
#include <memory>
#include <atomic>

namespace VoiceAssistant {

struct PipelineConfig {
    std::string hotword{"hey willow"};
    float kwsThreshold{0.25f};
    float speakerThreshold{0.65f};
    bool speakerVerificationEnabled{true};
    std::string enrolledUser{"owner"};
    float commandEndpointSilence{0.3f};
    float typingEndpointSilence{0.35f};
    int typingMaxBackspace{20};
    int typingCheckRecentChars{100};
    bool typingRealtime{true};
    TtsConfig ttsConfig;
    std::vector<std::string> kwsPhrases;
    std::vector<std::string> typingExitPhrases;
    std::vector<std::string> normalModePhrases;
    std::vector<std::string> typingModePhrases;
};

class SpeechPipeline {
public:
    using KeywordCallback = std::function<void(const std::string& keyword, bool fromKws)>;
    using TranscriptionCallback = std::function<void(const TranscriptionResult&)>;
    using PartialCallback = std::function<void(const std::string& partial, bool isFinal)>;
    using SpeakerFailCallback = std::function<void(const std::string& reason)>;
    using CommandPendingCallback = std::function<void(const std::string& phrase, bool blocked)>;

    explicit SpeechPipeline(std::shared_ptr<CommandExecutor> executor);
    ~SpeechPipeline();

    bool initialize(const std::string& modelBasePath, const PipelineConfig& config);
    void shutdown();
    bool isReady() const { return m_ready; }

    void processAudio(const std::vector<float>& chunk);
    void setMode(Mode mode);

    void setKeywordCallback(KeywordCallback cb) { m_keywordCallback = std::move(cb); }
    void setTranscriptionCallback(TranscriptionCallback cb) { m_transcriptionCallback = std::move(cb); }
    void setPartialCallback(PartialCallback cb) { m_partialCallback = std::move(cb); }
    void setSpeakerFailCallback(SpeakerFailCallback cb) { m_speakerFailCallback = std::move(cb); }
    void setCommandPendingCallback(CommandPendingCallback cb) { m_commandPendingCallback = std::move(cb); }

    void updateKeywords(const std::vector<std::string>& phrases);
    void updateCommands(const std::vector<Command>& commands);
    void setCommandThreshold(double threshold);
    void applyConfig(const PipelineConfig& config);

    bool typingRealtime() const { return m_config.typingRealtime; }

    KeywordSpotterEngine& kws() { return m_kws; }
    StreamingASREngine& asr() { return m_asr; }
    SpeakerVerifier& speaker() { return m_speaker; }
    TtsEngine& tts() { return m_tts; }
    CommandIntentResolver& commandResolver() { return *m_commandResolver; }
    TypingStreamWriter& typingWriter() { return *m_typingWriter; }
    AudioRouter& router() { return m_router; }

    void resetAsrStream();
    void speak(const std::string& text, bool forCommand = false, bool forMode = false,
               bool forSearch = false, bool forError = false);

private:
    std::shared_ptr<CommandExecutor> m_executor;
    ModelPaths m_modelPaths;
    PipelineConfig m_config;
    AudioRouter m_router;
    KeywordSpotterEngine m_kws;
    StreamingASREngine m_asr;
    SpeakerVerifier m_speaker;
    TtsEngine m_tts;
    std::unique_ptr<CommandIntentResolver> m_commandResolver;
    std::unique_ptr<TypingStreamWriter> m_typingWriter;

    std::atomic<Mode> m_mode{Mode::Normal};
    bool m_ready{false};
    std::string m_hotword;

    KeywordCallback m_keywordCallback;
    TranscriptionCallback m_transcriptionCallback;
    PartialCallback m_partialCallback;
    SpeakerFailCallback m_speakerFailCallback;
    CommandPendingCallback m_commandPendingCallback;

    void onKeywordDetected(const std::string& keyword);
    void onTranscription(const TranscriptionResult& result);
    void handleKeywordModeChange(const std::string& keyword);
    bool isModeControlKeyword(const std::string& keyword) const;
    void log(const std::string& level, const std::string& message);
};

} // namespace VoiceAssistant
