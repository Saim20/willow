#include "SpeechPipeline.hpp"
#include "Types.hpp"
#include <algorithm>
#include <iostream>

namespace VoiceAssistant {

SpeechPipeline::SpeechPipeline(std::shared_ptr<CommandExecutor> executor)
    : m_executor(std::move(executor))
    , m_modelPaths("")
    , m_commandResolver(std::make_unique<CommandIntentResolver>(m_executor))
    , m_typingWriter(std::make_unique<TypingStreamWriter>(m_executor)) {}

SpeechPipeline::~SpeechPipeline() {
    shutdown();
}

void SpeechPipeline::log(const std::string& level, const std::string& message) {
    std::cout << "[SpeechPipeline] [" << level << "] " << message << std::endl;
}

bool SpeechPipeline::initialize(const std::string& modelBasePath, const PipelineConfig& config) {
    shutdown();
    m_config = config;
    m_modelPaths = ModelPaths(modelBasePath);
    m_hotword = config.hotword;

    const bool kwsOk = m_kws.initialize(m_modelPaths, config.kwsThreshold);
    const bool asrOk = m_asr.initialize(m_modelPaths, 2, config.commandEndpointSilence);
    const bool speakerOk = m_speaker.initialize(m_modelPaths);
    const bool ttsOk = m_tts.initialize(m_modelPaths);

    m_speaker.setEnabled(config.speakerVerificationEnabled);
    m_speaker.setThreshold(config.speakerThreshold);
    m_speaker.setEnrolledUser(config.enrolledUser);
    m_tts.setConfig(config.ttsConfig);
    m_typingWriter->setMaxBackspace(config.typingMaxBackspace);
    m_typingWriter->setCheckRecentChars(config.typingCheckRecentChars);

    m_kws.setCallback([this](const std::string& kw) { onKeywordDetected(kw); });

    m_asr.setCallback([this](const TranscriptionResult& r) { onTranscription(r); });

    m_commandResolver->setPendingCallback([this](const std::string& phrase, bool blocked) {
        if (m_commandPendingCallback) {
            m_commandPendingCallback(phrase, blocked);
        }
    });

    // Wire audio router
    m_router.addConsumer([this](const std::vector<float>& chunk) {
        if (m_mode == Mode::Normal || m_kws.isEnabled()) {
            m_kws.processAudio(chunk);
        }
        if (m_mode == Mode::Command || m_mode == Mode::Typing) {
            m_asr.processAudio(chunk);
        }
    });

    if (!config.kwsPhrases.empty()) {
        updateKeywords(config.kwsPhrases);
    }

    m_ready = kwsOk && asrOk;
    log("INFO", std::string("Pipeline ready - KWS:") + (kwsOk ? "yes" : "no") +
               " ASR:" + (asrOk ? "yes" : "no") +
               " Speaker:" + (speakerOk ? "yes" : "no") +
               " TTS:" + (ttsOk ? "yes" : "no"));
    return m_ready;
}

void SpeechPipeline::shutdown() {
    m_kws.shutdown();
    m_asr.shutdown();
    m_speaker.shutdown();
    m_tts.shutdown();
    m_ready = false;
}

void SpeechPipeline::processAudio(const std::vector<float>& chunk) {
    if (!m_ready) return;
    m_router.processChunk(chunk);
}

void SpeechPipeline::setMode(Mode mode) {
    m_mode = mode;
    resetAsrStream();
    m_typingWriter->reset();

    if (mode == Mode::Normal) {
        m_asr.setEnabled(false);
        m_kws.setEnabled(true);
    } else {
        m_kws.setEnabled(true);
        m_asr.setEnabled(true);
        const float silence = (mode == Mode::Typing)
            ? m_config.typingEndpointSilence
            : m_config.commandEndpointSilence;
        m_asr.setEndpointSilence(silence);
    }
}

void SpeechPipeline::applyConfig(const PipelineConfig& config) {
    m_config = config;
    m_hotword = config.hotword;

    m_speaker.setEnabled(config.speakerVerificationEnabled);
    m_speaker.setThreshold(config.speakerThreshold);
    m_speaker.setEnrolledUser(config.enrolledUser);
    m_tts.setConfig(config.ttsConfig);
    m_typingWriter->setMaxBackspace(config.typingMaxBackspace);
    m_typingWriter->setCheckRecentChars(config.typingCheckRecentChars);
    m_kws.setThreshold(config.kwsThreshold);

    if (!config.kwsPhrases.empty()) {
        updateKeywords(config.kwsPhrases);
    }

    if (m_mode == Mode::Typing) {
        m_asr.setEndpointSilence(config.typingEndpointSilence);
    } else if (m_mode == Mode::Command) {
        m_asr.setEndpointSilence(config.commandEndpointSilence);
    }
}

void SpeechPipeline::updateKeywords(const std::vector<std::string>& phrases) {
    m_kws.setKeywords(phrases);
}

void SpeechPipeline::updateCommands(const std::vector<Command>& commands) {
    m_commandResolver->setCommands(commands);
}

void SpeechPipeline::setCommandThreshold(double threshold) {
    m_commandResolver->setThreshold(threshold);
}

void SpeechPipeline::resetAsrStream() {
    m_asr.resetStream();
    m_typingWriter->reset();
}

void SpeechPipeline::speak(const std::string& text, bool forCommand, bool forMode,
                           bool forSearch, bool forError) {
    if (!m_config.ttsConfig.enabled) return;
    if (forCommand && !m_config.ttsConfig.commandExecuted) return;
    if (forMode && !m_config.ttsConfig.modeChanged) return;
    if (forSearch && !m_config.ttsConfig.searchExecuted) return;
    if (forError && !m_config.ttsConfig.errors) return;
    m_tts.speakAsync(text);
}

bool SpeechPipeline::isModeControlKeyword(const std::string& keyword) const {
    const std::string norm = CommandIntentResolver::normalizeText(keyword);

    auto matches = [&](const std::vector<std::string>& phrases) {
        for (const auto& phrase : phrases) {
            if (norm == CommandIntentResolver::normalizeText(phrase)) return true;
        }
        return false;
    };

    return matches(m_config.typingExitPhrases)
        || matches(m_config.normalModePhrases)
        || matches(m_config.typingModePhrases);
}

void SpeechPipeline::handleKeywordModeChange(const std::string& keyword) {
    const std::string norm = CommandIntentResolver::normalizeText(keyword);

    auto matches = [&](const std::vector<std::string>& phrases) {
        for (const auto& phrase : phrases) {
            if (norm == CommandIntentResolver::normalizeText(phrase)) return true;
        }
        return false;
    };

    if (matches(m_config.typingExitPhrases) || matches(m_config.normalModePhrases)) {
        if (m_keywordCallback) m_keywordCallback("normal", true);
        return;
    }

    if (matches(m_config.typingModePhrases)) {
        if (m_keywordCallback) m_keywordCallback("typing", true);
    }
}

void SpeechPipeline::onKeywordDetected(const std::string& keyword) {
    const std::string norm = CommandIntentResolver::normalizeText(keyword);
    log("INFO", "Keyword detected: " + keyword);

    // Mode control keywords take priority
    if (isModeControlKeyword(keyword)) {
        resetAsrStream();
        handleKeywordModeChange(keyword);
        return;
    }

    // Hotword in normal mode - verify speaker
    if (m_mode == Mode::Normal) {
        const std::string hotwordNorm = CommandIntentResolver::normalizeText(m_hotword);
        if (norm == hotwordNorm || norm.find(hotwordNorm) != std::string::npos) {
            const auto audio = m_router.getRecentAudio(2.0f);
            if (m_speaker.isEnabled() && m_speaker.isEnrolled()) {
                m_speaker.verifyAsync(audio, [this, keyword](bool verified, const std::string& reason) {
                    if (verified) {
                        if (m_keywordCallback) m_keywordCallback("command", true);
                        speak("Command mode", false, true);
                    } else {
                        if (m_speakerFailCallback) m_speakerFailCallback(reason);
                        speak("Unrecognized speaker", false, false, false, true);
                    }
                });
            } else {
                if (m_keywordCallback) m_keywordCallback("command", true);
                speak("Command mode", false, true);
            }
            return;
        }
    }

    // Command mode - instant KWS command dispatch
    if (m_mode == Mode::Command) {
        auto result = m_commandResolver->processKeyword(keyword);
        if (result.handled) {
            resetAsrStream();
            if (m_keywordCallback) m_keywordCallback(keyword, true);
        }
    }
}

void SpeechPipeline::onTranscription(const TranscriptionResult& result) {
    if (m_transcriptionCallback) {
        m_transcriptionCallback(result);
    }
    if (m_partialCallback) {
        m_partialCallback(result.text, result.isFinal);
    }

    if (m_mode == Mode::Command) {
        if (!result.isEndpoint) {
            m_commandResolver->processPartial(result.text);
        }
    }
}

} // namespace VoiceAssistant
