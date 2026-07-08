#pragma once

#include <sdbus-c++/sdbus-c++.h>
#include <json/json.h>
#include <pulse/simple.h>
#include <pulse/error.h>
#include <string>
#include <vector>
#include <map>
#include <memory>
#include <thread>
#include <atomic>
#include <mutex>

#include "CommandExecutor.hpp"
#include "SpeechPipeline.hpp"
#include "ModeWorkers.hpp"
#include "Types.hpp"

namespace VoiceAssistant {

class VoiceAssistantService {
public:
    VoiceAssistantService(sdbus::IConnection& connection, std::string objectPath);
    ~VoiceAssistantService();

    // D-Bus Methods
    void SetMode(const std::string& mode);
    std::string GetMode();
    std::map<std::string, sdbus::Variant> GetStatus();
    std::string GetConfig();
    void UpdateConfig(const std::string& configJson);
    void SetConfigValue(const std::string& key, const sdbus::Variant& value);
    std::string GetCommands();
    void AddCommand(const std::string& name, const std::string& command,
                    const std::vector<std::string>& phrases);
    void RemoveCommand(const std::string& name);
    void Start();
    void Stop();
    void Restart();
    std::string GetBuffer();

    // Speaker enrollment
    void StartSpeakerEnrollment();
    void CancelSpeakerEnrollment();
    std::map<std::string, sdbus::Variant> GetSpeakerEnrollmentStatus();
    void RemoveSpeakerProfile();

    bool isAudioActive() const { return m_audioActive.load(); }

    // D-Bus Signals
    void emitModeChanged(const std::string& newMode, const std::string& oldMode);
    void emitBufferChanged(const std::string& buffer);
    void emitPartialBufferChanged(const std::string& partial, bool isFinal);
    void emitCommandPending(const std::string& phrase, bool blocked);
    void emitSpeakerVerificationFailed(const std::string& reason);
    void emitTtsStarted(const std::string& text);
    void emitTtsFinished();
    void emitCommandExecuted(const std::string& command, const std::string& phrase, double confidence);
    void emitStatusChanged(const std::map<std::string, sdbus::Variant>& status);
    void emitError(const std::string& message, const std::string& details);
    void emitNotification(const std::string& title, const std::string& message, const std::string& urgency);
    void emitConfigChanged(const std::string& config);

    bool IsRunning() const { return m_isRunning; }
    std::string CurrentMode() const { return modeToString(m_currentMode); }
    std::string CurrentBuffer() const;
    std::string Version() const { return "3.0.0"; }

private:
    bool startAudioCapture();
    void stopAudioCapture();
    void audioProcessingLoop();
    bool tryStartListening();
    void startListeningRetryLoop();
    void stopListeningRetryLoop();

    void handleTranscription(const TranscriptionResult& result);
    void handleKeyword(const std::string& keyword);
    void buildKwsPhrases();
    void applyPipelineSettings();
    std::vector<std::string> collectKwsPhrases() const;

    void loadConfig();
    void saveConfig();
    Json::Value configToJson() const;
    void jsonToConfig(const Json::Value& json);

    Mode stringToMode(const std::string& modeStr) const;
    std::string modeToString(Mode mode) const;

    void log(const std::string& level, const std::string& message);
    void updateModeWorkers();
    PipelineConfig buildPipelineConfig() const;

    sdbus::IConnection& m_connection;
    std::string m_objectPath;
    std::unique_ptr<sdbus::IObject> m_object;

    std::shared_ptr<CommandExecutor> m_executor;
    std::shared_ptr<SpeechPipeline> m_pipeline;

    std::unique_ptr<NormalModeWorker> m_normalWorker;
    std::unique_ptr<CommandModeWorker> m_commandWorker;
    std::unique_ptr<TypingModeWorker> m_typingWorker;
    ModeWorker* m_currentWorker;

    std::atomic<bool> m_isRunning;
    std::atomic<bool> m_audioActive;
    std::atomic<Mode> m_currentMode;
    mutable std::mutex m_modeMutex;

    std::vector<Command> m_commands;
    mutable std::mutex m_commandsMutex;

    // Configuration
    std::string m_hotword;
    double m_commandThreshold;
    bool m_speakerVerificationEnabled{true};
    float m_speakerThreshold{0.65f};
    std::string m_enrolledUser{"owner"};
    float m_kwsThreshold{0.25f};
    float m_commandEndpointSilence{0.3f};
    float m_typingEndpointSilence{0.35f};
    bool m_typingRealtime{true};
    int m_typingMaxBackspace{20};
    int m_typingCheckRecentChars{100};
    TtsConfig m_ttsConfig;
    std::vector<std::string> m_typingExitPhrases;
    std::string m_configPath;
    std::string m_modelPath;
    mutable std::mutex m_configMutex;

    std::thread m_audioThread;
    std::thread m_listeningRetryThread;
    std::atomic<bool> m_stopAudioThread;
    std::atomic<bool> m_stopListeningRetry;
    std::atomic<bool> m_listeningDesired{true};

    pa_simple* m_pulseAudio;

    std::string m_logFile;
    mutable std::mutex m_logMutex;

    std::atomic<uint64_t> m_transcriptionGeneration{0};
};

} // namespace VoiceAssistant
