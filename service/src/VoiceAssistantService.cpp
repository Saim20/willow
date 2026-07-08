#include "VoiceAssistantService.hpp"
#include <fstream>
#include <sstream>
#include <algorithm>
#include <ctime>
#include <iomanip>
#include <cstdlib>
#include <filesystem>
#include <chrono>

namespace fs = std::filesystem;

namespace VoiceAssistant {

VoiceAssistantService::VoiceAssistantService(sdbus::IConnection& connection, std::string objectPath)
    : m_connection(connection)
    , m_objectPath(std::move(objectPath))
    , m_currentWorker(nullptr)
    , m_isRunning(false)
    , m_audioActive(false)
    , m_currentMode(Mode::Normal)
    , m_hotword("hey willow")
    , m_commandThreshold(0.8)
    , m_stopAudioThread(false)
    , m_stopListeningRetry(false)
    , m_pulseAudio(nullptr)
{
    m_object = sdbus::createObject(m_connection, sdbus::ObjectPath(m_objectPath));
    const char* interfaceName = "com.github.saim.Willow";

    auto setModeCallback = [this](sdbus::MethodCall call) {
        std::string mode; call >> mode; SetMode(mode);
        call.createReply().send();
    };
    auto getModeCallback = [this](sdbus::MethodCall call) {
        auto reply = call.createReply(); reply << GetMode(); reply.send();
    };
    auto getStatusCallback = [this](sdbus::MethodCall call) {
        auto reply = call.createReply(); reply << GetStatus(); reply.send();
    };
    auto getConfigCallback = [this](sdbus::MethodCall call) {
        auto reply = call.createReply(); reply << GetConfig(); reply.send();
    };
    auto updateConfigCallback = [this](sdbus::MethodCall call) {
        std::string config; call >> config; UpdateConfig(config);
        call.createReply().send();
    };
    auto setConfigValueCallback = [this](sdbus::MethodCall call) {
        std::string key; sdbus::Variant value; call >> key >> value;
        SetConfigValue(key, value); call.createReply().send();
    };
    auto startCallback = [this](sdbus::MethodCall call) {
        Start(); call.createReply().send();
    };
    auto stopCallback = [this](sdbus::MethodCall call) {
        Stop(); call.createReply().send();
    };
    auto restartCallback = [this](sdbus::MethodCall call) {
        Restart(); call.createReply().send();
    };
    auto getBufferCallback = [this](sdbus::MethodCall call) {
        auto reply = call.createReply(); reply << GetBuffer(); reply.send();
    };
    auto getCommandsCallback = [this](sdbus::MethodCall call) {
        auto reply = call.createReply(); reply << GetCommands(); reply.send();
    };
    auto addCommandCallback = [this](sdbus::MethodCall call) {
        std::string name, command; std::vector<std::string> phrases;
        call >> name >> command >> phrases; AddCommand(name, command, phrases);
        call.createReply().send();
    };
    auto removeCommandCallback = [this](sdbus::MethodCall call) {
        std::string name; call >> name; RemoveCommand(name);
        call.createReply().send();
    };
    auto startEnrollCallback = [this](sdbus::MethodCall call) {
        StartSpeakerEnrollment(); call.createReply().send();
    };
    auto cancelEnrollCallback = [this](sdbus::MethodCall call) {
        CancelSpeakerEnrollment(); call.createReply().send();
    };
    auto getEnrollStatusCallback = [this](sdbus::MethodCall call) {
        auto reply = call.createReply(); reply << GetSpeakerEnrollmentStatus(); reply.send();
    };
    auto removeProfileCallback = [this](sdbus::MethodCall call) {
        RemoveSpeakerProfile(); call.createReply().send();
    };

    m_object->addVTable(
        sdbus::MethodVTableItem{sdbus::MethodName{"SetMode"}, sdbus::Signature{"s"}, {"mode"}, sdbus::Signature{""}, {}, setModeCallback, {}},
        sdbus::MethodVTableItem{sdbus::MethodName{"GetMode"}, sdbus::Signature{""}, {}, sdbus::Signature{"s"}, {"mode"}, getModeCallback, {}},
        sdbus::MethodVTableItem{sdbus::MethodName{"GetStatus"}, sdbus::Signature{""}, {}, sdbus::Signature{"a{sv}"}, {"status"}, getStatusCallback, {}},
        sdbus::MethodVTableItem{sdbus::MethodName{"GetConfig"}, sdbus::Signature{""}, {}, sdbus::Signature{"s"}, {"config"}, getConfigCallback, {}},
        sdbus::MethodVTableItem{sdbus::MethodName{"UpdateConfig"}, sdbus::Signature{"s"}, {"config"}, sdbus::Signature{""}, {}, updateConfigCallback, {}},
        sdbus::MethodVTableItem{sdbus::MethodName{"SetConfigValue"}, sdbus::Signature{"sv"}, {"key", "value"}, sdbus::Signature{""}, {}, setConfigValueCallback, {}},
        sdbus::MethodVTableItem{sdbus::MethodName{"GetCommands"}, sdbus::Signature{""}, {}, sdbus::Signature{"s"}, {"commands"}, getCommandsCallback, {}},
        sdbus::MethodVTableItem{sdbus::MethodName{"AddCommand"}, sdbus::Signature{"ssas"}, {"name", "command", "phrases"}, sdbus::Signature{""}, {}, addCommandCallback, {}},
        sdbus::MethodVTableItem{sdbus::MethodName{"RemoveCommand"}, sdbus::Signature{"s"}, {"name"}, sdbus::Signature{""}, {}, removeCommandCallback, {}},
        sdbus::MethodVTableItem{sdbus::MethodName{"Start"}, sdbus::Signature{""}, {}, sdbus::Signature{""}, {}, startCallback, {}},
        sdbus::MethodVTableItem{sdbus::MethodName{"Stop"}, sdbus::Signature{""}, {}, sdbus::Signature{""}, {}, stopCallback, {}},
        sdbus::MethodVTableItem{sdbus::MethodName{"Restart"}, sdbus::Signature{""}, {}, sdbus::Signature{""}, {}, restartCallback, {}},
        sdbus::MethodVTableItem{sdbus::MethodName{"GetBuffer"}, sdbus::Signature{""}, {}, sdbus::Signature{"s"}, {"buffer"}, getBufferCallback, {}},
        sdbus::MethodVTableItem{sdbus::MethodName{"StartSpeakerEnrollment"}, sdbus::Signature{""}, {}, sdbus::Signature{""}, {}, startEnrollCallback, {}},
        sdbus::MethodVTableItem{sdbus::MethodName{"CancelSpeakerEnrollment"}, sdbus::Signature{""}, {}, sdbus::Signature{""}, {}, cancelEnrollCallback, {}},
        sdbus::MethodVTableItem{sdbus::MethodName{"GetSpeakerEnrollmentStatus"}, sdbus::Signature{""}, {}, sdbus::Signature{"a{sv}"}, {"status"}, getEnrollStatusCallback, {}},
        sdbus::MethodVTableItem{sdbus::MethodName{"RemoveSpeakerProfile"}, sdbus::Signature{""}, {}, sdbus::Signature{""}, {}, removeProfileCallback, {}},

        sdbus::SignalVTableItem{sdbus::SignalName{"ModeChanged"}, sdbus::Signature{"ss"}, {"new_mode", "old_mode"}, {}},
        sdbus::SignalVTableItem{sdbus::SignalName{"StatusChanged"}, sdbus::Signature{"a{sv}"}, {"status"}, {}},
        sdbus::SignalVTableItem{sdbus::SignalName{"BufferChanged"}, sdbus::Signature{"s"}, {"buffer"}, {}},
        sdbus::SignalVTableItem{sdbus::SignalName{"PartialBufferChanged"}, sdbus::Signature{"sb"}, {"partial", "is_final"}, {}},
        sdbus::SignalVTableItem{sdbus::SignalName{"CommandPending"}, sdbus::Signature{"sb"}, {"phrase", "blocked_by_prefix"}, {}},
        sdbus::SignalVTableItem{sdbus::SignalName{"SpeakerVerificationFailed"}, sdbus::Signature{"s"}, {"reason"}, {}},
        sdbus::SignalVTableItem{sdbus::SignalName{"TtsStarted"}, sdbus::Signature{"s"}, {"text"}, {}},
        sdbus::SignalVTableItem{sdbus::SignalName{"TtsFinished"}, sdbus::Signature{""}, {}, {}},
        sdbus::SignalVTableItem{sdbus::SignalName{"ConfigChanged"}, sdbus::Signature{"s"}, {"config"}, {}},
        sdbus::SignalVTableItem{sdbus::SignalName{"CommandExecuted"}, sdbus::Signature{"ssd"}, {"command", "phrase", "confidence"}, {}},
        sdbus::SignalVTableItem{sdbus::SignalName{"Error"}, sdbus::Signature{"ss"}, {"message", "details"}, {}},
        sdbus::SignalVTableItem{sdbus::SignalName{"Notification"}, sdbus::Signature{"sss"}, {"title", "message", "urgency"}, {}},

        sdbus::registerProperty("IsRunning").withGetter([this]() { return m_isRunning.load() && m_audioActive.load(); }),
        sdbus::registerProperty("CurrentMode").withGetter([this]() { return CurrentMode(); }),
        sdbus::registerProperty("CurrentBuffer").withGetter([this]() { return CurrentBuffer(); }),
        sdbus::registerProperty("Version").withGetter([this]() { return Version(); })
    ).forInterface(interfaceName);

    const char* home = std::getenv("HOME");
    if (!home) {
        throw std::runtime_error("HOME is not set; cannot locate Willow config and models");
    }
    m_configPath = std::string(home) + "/.config/willow/config.json";
    m_logFile = "/tmp/willow.log";
    m_modelPath = std::string(home) + "/.local/share/willow/models";

    loadConfig();

    m_executor = std::make_shared<CommandExecutor>();
    m_pipeline = std::make_shared<SpeechPipeline>(m_executor);

    if (!m_pipeline->initialize(m_modelPath, buildPipelineConfig())) {
        log("ERROR", "Failed to initialize speech pipeline - run willow-download-model");
        emitError("Initialization Error", "Failed to load sherpa-onnx models from: " + m_modelPath);
    }

    m_pipeline->setKeywordCallback([this](const std::string& kw, bool) { handleKeyword(kw); });
    m_pipeline->setTranscriptionCallback([this](const TranscriptionResult& r) { handleTranscription(r); });
    m_pipeline->setPartialCallback([this](const std::string& partial, bool isFinal) {
        emitPartialBufferChanged(partial, isFinal);
        if (m_currentMode == Mode::Typing || m_currentMode == Mode::Command) {
            emitBufferChanged(partial);
        }
    });
    m_pipeline->setSpeakerFailCallback([this](const std::string& reason) {
        emitSpeakerVerificationFailed(reason);
    });
    m_pipeline->setCommandPendingCallback([this](const std::string& phrase, bool blocked) {
        emitCommandPending(phrase, blocked);
    });
    m_pipeline->tts().setCallback([this](const std::string& text, bool started) {
        if (started) emitTtsStarted(text);
        else emitTtsFinished();
    });

    m_normalWorker = std::make_unique<NormalModeWorker>(m_executor, m_pipeline);
    m_commandWorker = std::make_unique<CommandModeWorker>(m_executor, m_pipeline);
    m_typingWorker = std::make_unique<TypingModeWorker>(m_executor, m_pipeline);

    auto modeChangeCallback = [this](const std::string& newMode) { SetMode(newMode); };
    m_normalWorker->setModeChangeCallback(modeChangeCallback);
    m_commandWorker->setModeChangeCallback(modeChangeCallback);
    m_typingWorker->setModeChangeCallback(modeChangeCallback);

    m_commandWorker->setCommandExecutedCallback(
        [this](const std::string& cmd, const std::string& phrase, double conf) {
            emitCommandExecuted(cmd, phrase, conf);
        });

    m_normalWorker->setHotword(m_hotword);
    m_commandWorker->setCommands(m_commands);
    m_commandWorker->setThreshold(m_commandThreshold);
    m_typingWorker->setExitPhrases(m_typingExitPhrases);

    m_currentWorker = m_normalWorker.get();
    m_pipeline->setMode(Mode::Normal);

    log("INFO", "Voice Assistant Service initialized (sherpa-onnx pipeline v3)");
    startListeningRetryLoop();

    if (m_pipeline->isReady()) {
        emitStatusChanged(GetStatus());
        tryStartListening();
    } else {
        log("WARNING", "Speech models not loaded - run willow-download-model then restart");
        emitStatusChanged(GetStatus());
    }
}

VoiceAssistantService::~VoiceAssistantService() {
    stopListeningRetryLoop();
    Stop();
    m_pipeline->shutdown();
}

PipelineConfig VoiceAssistantService::buildPipelineConfig() const {
    PipelineConfig cfg;
    cfg.hotword = m_hotword;
    cfg.kwsThreshold = m_kwsThreshold;
    cfg.speakerThreshold = m_speakerThreshold;
    cfg.speakerVerificationEnabled = m_speakerVerificationEnabled;
    cfg.enrolledUser = m_enrolledUser;
    cfg.commandEndpointSilence = m_commandEndpointSilence;
    cfg.typingEndpointSilence = m_typingEndpointSilence;
    cfg.typingMaxBackspace = m_typingMaxBackspace;
    cfg.typingCheckRecentChars = m_typingCheckRecentChars;
    cfg.typingRealtime = m_typingRealtime;
    cfg.ttsConfig = m_ttsConfig;
    cfg.typingExitPhrases = m_typingExitPhrases;
    cfg.kwsPhrases = collectKwsPhrases();

    for (const auto& cmd : m_commands) {
        if (cmd.command == "exit_command_mode") {
            cfg.normalModePhrases.insert(cfg.normalModePhrases.end(),
                                         cmd.phrases.begin(), cmd.phrases.end());
        } else if (cmd.command == "start_typing_mode") {
            cfg.typingModePhrases.insert(cfg.typingModePhrases.end(),
                                         cmd.phrases.begin(), cmd.phrases.end());
        }
    }
    return cfg;
}

void VoiceAssistantService::applyPipelineSettings() {
    if (!m_pipeline) return;
    m_pipeline->applyConfig(buildPipelineConfig());
    m_pipeline->setCommandThreshold(m_commandThreshold);
}

std::vector<std::string> VoiceAssistantService::collectKwsPhrases() const {
    std::vector<std::string> phrases;
    phrases.push_back(m_hotword);
    for (const auto& phrase : m_typingExitPhrases) phrases.push_back(phrase);
    for (const auto& cmd : m_commands) {
        if (cmd.command == "exit_command_mode" || cmd.command == "start_typing_mode") {
            for (const auto& p : cmd.phrases) phrases.push_back(p);
        }
    }
    return phrases;
}

void VoiceAssistantService::buildKwsPhrases() {
    applyPipelineSettings();
}

void VoiceAssistantService::SetMode(const std::string& mode) {
    Mode newMode = stringToMode(mode);
    std::string oldModeStr = modeToString(m_currentMode);

    std::lock_guard<std::mutex> lock(m_modeMutex);

    if (m_currentWorker) m_currentWorker->stop();

    m_currentMode = newMode;
    m_pipeline->setMode(newMode);
    updateModeWorkers();

    ++m_transcriptionGeneration;
    emitModeChanged(mode, oldModeStr);
    emitStatusChanged(GetStatus());
    m_pipeline->speak(modeToString(newMode) + " mode", false, true);
    log("INFO", "Mode changed from " + oldModeStr + " to " + mode);
}

std::string VoiceAssistantService::GetMode() { return modeToString(m_currentMode); }

std::map<std::string, sdbus::Variant> VoiceAssistantService::GetStatus() {
    std::map<std::string, sdbus::Variant> status;
    status["is_running"] = sdbus::Variant(m_isRunning.load() && m_audioActive.load());
    status["audio_active"] = sdbus::Variant(m_audioActive.load());
    status["current_mode"] = sdbus::Variant(modeToString(m_currentMode));
    status["current_buffer"] = sdbus::Variant(GetBuffer());
    status["command_count"] = sdbus::Variant(static_cast<int32_t>(m_commands.size()));
    status["models_loaded"] = sdbus::Variant(m_pipeline->isReady());
    status["whisper_loaded"] = sdbus::Variant(m_pipeline->isReady()); // compat
    status["kws_active"] = sdbus::Variant(m_pipeline->kws().isLoaded());
    status["speaker_enrolled"] = sdbus::Variant(m_pipeline->speaker().isEnrolled());
    status["streaming_active"] = sdbus::Variant(m_pipeline->asr().isLoaded());
    status["tts_enabled"] = sdbus::Variant(m_ttsConfig.enabled);
    return status;
}

std::string VoiceAssistantService::GetConfig() {
    std::lock_guard<std::mutex> lock(m_configMutex);
    Json::StreamWriterBuilder writer;
    return Json::writeString(writer, configToJson());
}

void VoiceAssistantService::UpdateConfig(const std::string& configJson) {
    std::lock_guard<std::mutex> lock(m_configMutex);
    Json::CharReaderBuilder reader;
    Json::Value root;
    std::string errs;
    std::istringstream stream(configJson);

    if (Json::parseFromStream(reader, stream, &root, &errs)) {
        jsonToConfig(root);
        saveConfig();
        applyPipelineSettings();
        m_normalWorker->setHotword(m_hotword);
        m_commandWorker->setCommands(m_commands);
        m_commandWorker->setThreshold(m_commandThreshold);
        m_typingWorker->setExitPhrases(m_typingExitPhrases);
        emitConfigChanged(configJson);
        emitStatusChanged(GetStatus());
        log("INFO", "Configuration updated via D-Bus");
    } else {
        emitError("Configuration Error", "Failed to parse JSON: " + errs);
    }
}

void VoiceAssistantService::SetConfigValue(const std::string& key, const sdbus::Variant& value) {
    std::lock_guard<std::mutex> lock(m_configMutex);
    if (key == "hotword") {
        m_hotword = value.get<std::string>();
        m_normalWorker->setHotword(m_hotword);
    } else if (key == "command_threshold") {
        double threshold = value.get<double>();
        if (threshold > 1.0) threshold /= 100.0;
        m_commandThreshold = threshold;
        m_commandWorker->setThreshold(m_commandThreshold);
    }
    applyPipelineSettings();
    saveConfig();
    log("INFO", "Config value updated: " + key);
}

std::string VoiceAssistantService::GetCommands() {
    std::lock_guard<std::mutex> lock(m_commandsMutex);
    Json::Value root(Json::arrayValue);
    for (const auto& cmd : m_commands) {
        Json::Value cmdJson;
        cmdJson["name"] = cmd.name;
        cmdJson["command"] = cmd.command;
        Json::Value phrases(Json::arrayValue);
        for (const auto& phrase : cmd.phrases) phrases.append(phrase);
        cmdJson["phrases"] = phrases;
        root.append(cmdJson);
    }
    Json::StreamWriterBuilder writer;
    return Json::writeString(writer, root);
}

void VoiceAssistantService::AddCommand(const std::string& name, const std::string& command,
                                       const std::vector<std::string>& phrases) {
    std::lock_guard<std::mutex> lock(m_commandsMutex);
    m_commands.erase(
        std::remove_if(m_commands.begin(), m_commands.end(),
            [&name](const Command& c) { return c.name == name; }),
        m_commands.end());
    m_commands.push_back({name, command, phrases});
    saveConfig();
    buildKwsPhrases();
    applyPipelineSettings();
    m_commandWorker->setCommands(m_commands);
    log("INFO", "Command added: " + name);
    emitStatusChanged(GetStatus());
}

void VoiceAssistantService::RemoveCommand(const std::string& name) {
    std::lock_guard<std::mutex> lock(m_commandsMutex);
    auto it = std::remove_if(m_commands.begin(), m_commands.end(),
        [&name](const Command& c) { return c.name == name; });
    if (it != m_commands.end()) {
        m_commands.erase(it, m_commands.end());
        saveConfig();
        buildKwsPhrases();
        m_commandWorker->setCommands(m_commands);
        log("INFO", "Command removed: " + name);
        emitStatusChanged(GetStatus());
    }
}

void VoiceAssistantService::Start() {
    if (m_isRunning && m_audioActive) return;
    if (!m_pipeline->isReady()) {
        emitError("Start Error", "Speech models not loaded");
        return;
    }
    m_listeningDesired = true;
    tryStartListening();
}

bool VoiceAssistantService::tryStartListening() {
    if (!m_pipeline->isReady()) return false;
    if (m_audioActive) return true;

    if (m_isRunning) {
        m_stopAudioThread = true;
        stopAudioCapture();
        if (m_currentWorker) m_currentWorker->stop();
        m_isRunning = false;
        m_audioActive = false;
    }

    m_stopAudioThread = false;
    if (m_currentWorker) m_currentWorker->start();

    if (!startAudioCapture()) {
        if (m_currentWorker) m_currentWorker->stop();
        log("WARNING", "Audio capture unavailable, will retry automatically");
        return false;
    }

    m_isRunning = true;
    m_audioActive = true;
    log("INFO", "Voice Assistant started");
    emitNotification("Voice Assistant", "Service started", "normal");
    emitStatusChanged(GetStatus());
    return true;
}

void VoiceAssistantService::Stop() {
    if (!m_isRunning && !m_audioActive) return;
    m_listeningDesired = false;
    m_isRunning = false;
    m_audioActive = false;
    m_stopAudioThread = true;
    if (m_currentWorker) m_currentWorker->stop();
    stopAudioCapture();
    log("INFO", "Voice Assistant stopped");
    emitNotification("Voice Assistant", "Service stopped", "normal");
    emitStatusChanged(GetStatus());
}

void VoiceAssistantService::Restart() {
    m_listeningDesired = true;
    Stop();
    std::this_thread::sleep_for(std::chrono::milliseconds(500));
    tryStartListening();
}

std::string VoiceAssistantService::GetBuffer() {
    return m_currentWorker ? m_currentWorker->getBuffer() : "";
}

std::string VoiceAssistantService::CurrentBuffer() const {
    return m_currentWorker ? m_currentWorker->getBuffer() : "";
}

void VoiceAssistantService::StartSpeakerEnrollment() {
    m_pipeline->speaker().startEnrollment(m_enrolledUser);
    log("INFO", "Speaker enrollment started");
    emitStatusChanged(GetStatus());
}

void VoiceAssistantService::CancelSpeakerEnrollment() {
    m_pipeline->speaker().cancelEnrollment();
    emitStatusChanged(GetStatus());
}

std::map<std::string, sdbus::Variant> VoiceAssistantService::GetSpeakerEnrollmentStatus() {
    std::map<std::string, sdbus::Variant> status;
    const auto state = m_pipeline->speaker().enrollmentState();
    status["state"] = sdbus::Variant(
        state == EnrollmentState::Recording ? "recording" :
        state == EnrollmentState::Complete ? "complete" :
        state == EnrollmentState::Failed ? "failed" : "idle");
    status["samples"] = sdbus::Variant(m_pipeline->speaker().enrollmentProgress());
    status["enrolled"] = sdbus::Variant(m_pipeline->speaker().isEnrolled());
    return status;
}

void VoiceAssistantService::RemoveSpeakerProfile() {
    m_pipeline->speaker().removeProfile();
    emitStatusChanged(GetStatus());
}

// Signal emission

void VoiceAssistantService::emitModeChanged(const std::string& newMode, const std::string& oldMode) {
    m_object->emitSignal("ModeChanged").onInterface("com.github.saim.Willow")
        .withArguments(newMode, oldMode);
}

void VoiceAssistantService::emitBufferChanged(const std::string& buffer) {
    m_object->emitSignal("BufferChanged").onInterface("com.github.saim.Willow")
        .withArguments(buffer);
}

void VoiceAssistantService::emitPartialBufferChanged(const std::string& partial, bool isFinal) {
    m_object->emitSignal("PartialBufferChanged").onInterface("com.github.saim.Willow")
        .withArguments(partial, isFinal);
}

void VoiceAssistantService::emitCommandPending(const std::string& phrase, bool blocked) {
    m_object->emitSignal("CommandPending").onInterface("com.github.saim.Willow")
        .withArguments(phrase, blocked);
}

void VoiceAssistantService::emitSpeakerVerificationFailed(const std::string& reason) {
    m_object->emitSignal("SpeakerVerificationFailed").onInterface("com.github.saim.Willow")
        .withArguments(reason);
}

void VoiceAssistantService::emitTtsStarted(const std::string& text) {
    m_object->emitSignal("TtsStarted").onInterface("com.github.saim.Willow")
        .withArguments(text);
}

void VoiceAssistantService::emitTtsFinished() {
    m_object->emitSignal("TtsFinished").onInterface("com.github.saim.Willow");
}

void VoiceAssistantService::emitCommandExecuted(const std::string& command,
                                                const std::string& phrase, double confidence) {
    m_object->emitSignal("CommandExecuted").onInterface("com.github.saim.Willow")
        .withArguments(command, phrase, confidence);
}

void VoiceAssistantService::emitStatusChanged(const std::map<std::string, sdbus::Variant>& status) {
    m_object->emitSignal("StatusChanged").onInterface("com.github.saim.Willow")
        .withArguments(status);
}

void VoiceAssistantService::emitError(const std::string& message, const std::string& details) {
    m_object->emitSignal("Error").onInterface("com.github.saim.Willow")
        .withArguments(message, details);
}

void VoiceAssistantService::emitNotification(const std::string& title,
                                             const std::string& message,
                                             const std::string& urgency) {
    m_object->emitSignal("Notification").onInterface("com.github.saim.Willow")
        .withArguments(title, message, urgency);
}

void VoiceAssistantService::emitConfigChanged(const std::string& config) {
    m_object->emitSignal("ConfigChanged").onInterface("com.github.saim.Willow")
        .withArguments(config);
}

bool VoiceAssistantService::startAudioCapture() {
    pa_sample_spec ss;
    ss.format = PA_SAMPLE_FLOAT32LE;
    ss.channels = 1;
    ss.rate = 16000;

    pa_buffer_attr bufattr;
    bufattr.maxlength = (uint32_t)-1;
    bufattr.fragsize = 4096;

    int error = 0;
    m_pulseAudio = pa_simple_new(nullptr, "Willow", PA_STREAM_RECORD, nullptr,
                                 "Voice Input", &ss, nullptr, &bufattr, &error);
    if (!m_pulseAudio) {
        emitError("Audio Error", "Failed to connect to PulseAudio: " + std::string(pa_strerror(error)));
        return false;
    }

    m_audioThread = std::thread(&VoiceAssistantService::audioProcessingLoop, this);
    return true;
}

void VoiceAssistantService::stopAudioCapture() {
    if (m_audioThread.joinable()) {
        m_stopAudioThread = true;
        m_audioThread.join();
    }
    if (m_pulseAudio) {
        pa_simple_free(m_pulseAudio);
        m_pulseAudio = nullptr;
    }
}

void VoiceAssistantService::audioProcessingLoop() {
    const size_t CHUNK_SIZE = 4096;
    std::vector<float> chunk(CHUNK_SIZE);
    int error = 0;

    while (!m_stopAudioThread) {
        if (pa_simple_read(m_pulseAudio, chunk.data(), chunk.size() * sizeof(float), &error) < 0) {
            emitError("Audio Error", "Failed to read audio: " + std::string(pa_strerror(error)));
            break;
        }

        m_pipeline->processAudio(chunk);

        // Feed enrollment audio if active
        if (m_pipeline->speaker().enrollmentState() == EnrollmentState::Recording) {
            m_pipeline->speaker().addEnrollmentAudio(chunk);
            if (m_pipeline->speaker().enrollmentProgress() >= 3) {
                m_pipeline->speaker().finishEnrollment();
                emitStatusChanged(GetStatus());
            }
        }

        if (m_stopAudioThread) break;
    }

    if (!m_stopAudioThread) {
        m_audioActive = false;
        m_isRunning = false;
        if (m_pulseAudio) { pa_simple_free(m_pulseAudio); m_pulseAudio = nullptr; }
        emitStatusChanged(GetStatus());
    }
}

void VoiceAssistantService::handleTranscription(const TranscriptionResult& result) {
    if (!m_currentWorker || !m_isRunning || !m_audioActive) return;

    // No debounce for streaming pipeline - process immediately
    if (m_currentMode == Mode::Normal) return;

    m_currentWorker->processTranscription(result);

    const std::string buffer = GetBuffer();
    if (!buffer.empty()) {
        emitBufferChanged(buffer);
    }
}

void VoiceAssistantService::handleKeyword(const std::string& keyword) {
    if (!m_isRunning || !m_audioActive) return;

    if (keyword == "command") {
        m_normalWorker->processKeyword(keyword);
        return;
    }
    if (keyword == "normal" || keyword == "typing") {
        if (m_currentWorker) m_currentWorker->processKeyword(keyword);
        return;
    }

    if (m_currentWorker) {
        m_currentWorker->processKeyword(keyword);
    }
}

void VoiceAssistantService::updateModeWorkers() {
    switch (m_currentMode.load()) {
        case Mode::Normal: m_currentWorker = m_normalWorker.get(); break;
        case Mode::Command: m_currentWorker = m_commandWorker.get(); break;
        case Mode::Typing: m_currentWorker = m_typingWorker.get(); break;
    }
    if (m_isRunning && m_currentWorker) m_currentWorker->start();
}

void VoiceAssistantService::loadConfig() {
    std::lock_guard<std::mutex> lock(m_configMutex);
    if (!fs::exists(m_configPath)) {
        const std::string systemConfig = "/usr/share/willow/config.json";
        if (fs::exists(systemConfig)) {
            try {
                fs::create_directories(fs::path(m_configPath).parent_path());
                fs::copy_file(systemConfig, m_configPath);
            } catch (...) {}
        } else {
            return;
        }
    }

    std::ifstream file(m_configPath);
    if (!file) return;

    Json::CharReaderBuilder reader;
    Json::Value root;
    std::string errs;
    if (Json::parseFromStream(reader, file, &root, &errs)) {
        jsonToConfig(root);
    }
}

void VoiceAssistantService::saveConfig() {
    Json::Value root = configToJson();
    fs::create_directories(fs::path(m_configPath).parent_path());
    std::ofstream file(m_configPath);
    if (file) {
        Json::StreamWriterBuilder writer;
        writer["indentation"] = "  ";
        file << Json::writeString(writer, root);
    }
}

Json::Value VoiceAssistantService::configToJson() const {
    Json::Value root;
    root["hotword"] = m_hotword;
    root["command_threshold"] = m_commandThreshold * 100.0;

    Json::Value speaker;
    speaker["enabled"] = m_speakerVerificationEnabled;
    speaker["threshold"] = m_speakerThreshold;
    speaker["enrolled_user"] = m_enrolledUser;
    root["speaker_verification"] = speaker;

    Json::Value kws;
    kws["threshold"] = m_kwsThreshold;
    root["kws"] = kws;

    Json::Value streaming;
    streaming["endpoint_silence_command"] = m_commandEndpointSilence;
    streaming["endpoint_silence_typing"] = m_typingEndpointSilence;
    root["streaming_asr"] = streaming;

    Json::Value typing;
    typing["realtime"] = m_typingRealtime;
    typing["max_backspace"] = m_typingMaxBackspace;
    typing["check_recent_chars"] = m_typingCheckRecentChars;
    Json::Value exitPhrases(Json::arrayValue);
    for (const auto& p : m_typingExitPhrases) exitPhrases.append(p);
    typing["exit_phrases"] = exitPhrases;
    root["typing_mode"] = typing;

    Json::Value cmdMode;
    cmdMode["endpoint_silence"] = m_commandEndpointSilence;
    root["command_mode"] = cmdMode;

    Json::Value tts;
    tts["enabled"] = m_ttsConfig.enabled;
    Json::Value ttsEvents;
    ttsEvents["command_executed"] = m_ttsConfig.commandExecuted;
    ttsEvents["mode_changed"] = m_ttsConfig.modeChanged;
    ttsEvents["search_executed"] = m_ttsConfig.searchExecuted;
    ttsEvents["errors"] = m_ttsConfig.errors;
    tts["events"] = ttsEvents;
    root["tts"] = tts;

    Json::Value commands(Json::arrayValue);
    for (const auto& cmd : m_commands) {
        Json::Value cmdJson;
        cmdJson["name"] = cmd.name;
        cmdJson["command"] = cmd.command;
        Json::Value phrases(Json::arrayValue);
        for (const auto& p : cmd.phrases) phrases.append(p);
        cmdJson["phrases"] = phrases;
        commands.append(cmdJson);
    }
    root["commands"] = commands;
    return root;
}

void VoiceAssistantService::jsonToConfig(const Json::Value& json) {
    if (json.isMember("hotword")) m_hotword = json["hotword"].asString();
    if (json.isMember("command_threshold")) m_commandThreshold = json["command_threshold"].asDouble() / 100.0;

    if (json.isMember("speaker_verification")) {
        const auto& sv = json["speaker_verification"];
        if (sv.isMember("enabled")) m_speakerVerificationEnabled = sv["enabled"].asBool();
        if (sv.isMember("threshold")) m_speakerThreshold = static_cast<float>(sv["threshold"].asDouble());
        if (sv.isMember("enrolled_user")) m_enrolledUser = sv["enrolled_user"].asString();
    }

    if (json.isMember("kws") && json["kws"].isMember("threshold")) {
        m_kwsThreshold = static_cast<float>(json["kws"]["threshold"].asDouble());
    }

    if (json.isMember("streaming_asr")) {
        const auto& sa = json["streaming_asr"];
        if (sa.isMember("endpoint_silence_command")) m_commandEndpointSilence = static_cast<float>(sa["endpoint_silence_command"].asDouble());
        if (sa.isMember("endpoint_silence_typing")) m_typingEndpointSilence = static_cast<float>(sa["endpoint_silence_typing"].asDouble());
    }

    if (json.isMember("command_mode") && json["command_mode"].isMember("endpoint_silence")) {
        m_commandEndpointSilence = static_cast<float>(json["command_mode"]["endpoint_silence"].asDouble());
    }

    if (json.isMember("typing_mode")) {
        const auto& tm = json["typing_mode"];
        if (tm.isMember("realtime")) m_typingRealtime = tm["realtime"].asBool();
        if (tm.isMember("max_backspace")) m_typingMaxBackspace = tm["max_backspace"].asInt();
        if (tm.isMember("check_recent_chars")) m_typingCheckRecentChars = tm["check_recent_chars"].asInt();
        if (tm.isMember("exit_phrases") && tm["exit_phrases"].isArray()) {
            m_typingExitPhrases.clear();
            for (const auto& p : tm["exit_phrases"]) {
                std::string phrase = p.asString();
                std::transform(phrase.begin(), phrase.end(), phrase.begin(), ::tolower);
                m_typingExitPhrases.push_back(phrase);
            }
        }
    }

    if (json.isMember("tts")) {
        const auto& tts = json["tts"];
        if (tts.isMember("enabled")) m_ttsConfig.enabled = tts["enabled"].asBool();
        if (tts.isMember("events")) {
            const auto& ev = tts["events"];
            if (ev.isMember("command_executed")) m_ttsConfig.commandExecuted = ev["command_executed"].asBool();
            if (ev.isMember("mode_changed")) m_ttsConfig.modeChanged = ev["mode_changed"].asBool();
            if (ev.isMember("search_executed")) m_ttsConfig.searchExecuted = ev["search_executed"].asBool();
            if (ev.isMember("errors")) m_ttsConfig.errors = ev["errors"].asBool();
        }
    }

    if (json.isMember("commands") && json["commands"].isArray()) {
        std::lock_guard<std::mutex> lock(m_commandsMutex);
        m_commands.clear();
        for (const auto& cmdJson : json["commands"]) {
            bool isCommentOnly = true;
            for (const auto& key : cmdJson.getMemberNames()) {
                if (!key.empty() && key[0] != '_') { isCommentOnly = false; break; }
            }
            if (isCommentOnly) continue;
            Command cmd;
            cmd.name = cmdJson["name"].asString();
            cmd.command = cmdJson["command"].asString();
            if (cmdJson.isMember("phrases") && cmdJson["phrases"].isArray()) {
                for (const auto& p : cmdJson["phrases"]) cmd.phrases.push_back(p.asString());
            }
            m_commands.push_back(cmd);
        }
    }
}

Mode VoiceAssistantService::stringToMode(const std::string& modeStr) const {
    if (modeStr == "command") return Mode::Command;
    if (modeStr == "typing") return Mode::Typing;
    return Mode::Normal;
}

std::string VoiceAssistantService::modeToString(Mode mode) const {
    switch (mode) {
        case Mode::Command: return "command";
        case Mode::Typing: return "typing";
        default: return "normal";
    }
}

void VoiceAssistantService::log(const std::string& level, const std::string& message) {
    std::lock_guard<std::mutex> lock(m_logMutex);
    auto now = std::time(nullptr);
    auto tm = *std::localtime(&now);
    std::ofstream logFile(m_logFile, std::ios::app);
    if (logFile) {
        logFile << std::put_time(&tm, "%Y-%m-%d %H:%M:%S")
                << " [" << level << "] " << message << std::endl;
    }
}

void VoiceAssistantService::startListeningRetryLoop() {
    m_stopListeningRetry = false;
    m_listeningRetryThread = std::thread([this]() {
        while (!m_stopListeningRetry) {
            if (m_listeningDesired && m_pipeline && m_pipeline->isReady() && !m_audioActive) {
                tryStartListening();
            }
            for (int i = 0; i < 30 && !m_stopListeningRetry; ++i) {
                std::this_thread::sleep_for(std::chrono::milliseconds(100));
            }
        }
    });
}

void VoiceAssistantService::stopListeningRetryLoop() {
    m_stopListeningRetry = true;
    if (m_listeningRetryThread.joinable()) m_listeningRetryThread.join();
}

} // namespace VoiceAssistant
