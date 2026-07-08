#include "ModeWorkers.hpp"
#include <algorithm>

namespace VoiceAssistant {

namespace {

void notifyCommandExecuted(const CommandModeWorker::CommandExecutedCallback& callback,
                           const std::string& command,
                           const std::string& phrase,
                           double confidence) {
    if (callback) {
        callback(command, phrase, confidence);
    }
}

} // namespace

// NormalModeWorker

NormalModeWorker::NormalModeWorker(
    std::shared_ptr<CommandExecutor> executor,
    std::shared_ptr<SpeechPipeline> pipeline)
    : m_hotword("hey willow") {
    m_executor = executor;
    m_pipeline = pipeline;
}

void NormalModeWorker::start() {
    m_isRunning = true;
    m_executor->log("INFO", "Normal mode worker started (KWS hotword: " + m_hotword + ")");
}

void NormalModeWorker::stop() {
    m_isRunning = false;
    m_executor->log("INFO", "Normal mode worker stopped");
}

void NormalModeWorker::processTranscription(const TranscriptionResult&) {
    // Normal mode uses KWS only - no ASR transcription
}

void NormalModeWorker::processKeyword(const std::string& keyword) {
    if (!m_isRunning) return;
    if (keyword == "command") {
        requestModeChange("command");
    }
}

// CommandModeWorker

CommandModeWorker::CommandModeWorker(
    std::shared_ptr<CommandExecutor> executor,
    std::shared_ptr<SpeechPipeline> pipeline) {
    m_executor = executor;
    m_pipeline = pipeline;
}

void CommandModeWorker::start() {
    m_isRunning = true;
    {
        std::lock_guard<std::mutex> lock(m_bufferMutex);
        m_buffer.clear();
    }
    m_pipeline->resetAsrStream();
    m_executor->log("INFO", "Command mode worker started");
}

void CommandModeWorker::stop() {
    m_isRunning = false;
    {
        std::lock_guard<std::mutex> lock(m_bufferMutex);
        m_buffer.clear();
    }
    m_executor->log("INFO", "Command mode worker stopped");
}

void CommandModeWorker::setCommands(const std::vector<Command>& commands) {
    m_pipeline->updateCommands(commands);
}

void CommandModeWorker::setThreshold(double threshold) {
    m_pipeline->setCommandThreshold(threshold);
}

std::string CommandModeWorker::getBuffer() const {
    std::lock_guard<std::mutex> lock(m_bufferMutex);
    return m_buffer;
}

void CommandModeWorker::processTranscription(const TranscriptionResult& result) {
    if (!m_isRunning) return;

    {
        std::lock_guard<std::mutex> lock(m_bufferMutex);
        m_buffer = result.text;
    }

    if (!result.isEndpoint) return;

    auto dispatch = m_pipeline->commandResolver().processEndpoint(result.text);
    dispatchResult(dispatch);
    m_pipeline->resetAsrStream();
}

void CommandModeWorker::processKeyword(const std::string& keyword) {
    if (!m_isRunning) return;

    if (keyword == "normal") {
        requestModeChange("normal");
        return;
    }
    if (keyword == "typing") {
        requestModeChange("typing");
        return;
    }

    auto dispatch = m_pipeline->commandResolver().processKeyword(keyword);
    dispatchResult(dispatch);
    m_pipeline->resetAsrStream();
}

void CommandModeWorker::dispatchResult(const CommandDispatchResult& result) {
    if (!result.handled) {
        if (result.pending) return;
        m_pipeline->speak("Sorry, I didn't understand that", false, false, false, true);
        return;
    }
    executeDispatch(result);
}

void CommandModeWorker::executeDispatch(const CommandDispatchResult& result) {
    if (result.isSearch) {
        if (m_executor->executeSmartSearch(result.searchEngine, result.searchQuery)) {
            m_pipeline->speak("Searching " + result.searchEngine + " for " + result.searchQuery,
                                false, false, true);
            notifyCommandExecuted(m_commandExecutedCallback, result.searchEngine,
                                  result.matchedPhrase, result.confidence);
        }
        return;
    }

    if (result.isSmartOpen) {
        if (m_executor->executeSmartOpen(result.appName)) {
            m_pipeline->speak("Opening " + result.appName, true);
            notifyCommandExecuted(m_commandExecutedCallback, result.appName,
                                  result.matchedPhrase, result.confidence);
        }
        return;
    }

    if (result.commandAction == "exit_command_mode") {
        requestModeChange("normal");
        m_pipeline->speak("Normal mode", false, true);
        return;
    }
    if (result.commandAction == "start_typing_mode") {
        requestModeChange("typing");
        m_pipeline->speak("Typing mode", false, true);
        return;
    }

    m_executor->executeCommand(result.commandAction);
    m_pipeline->speak("Done", true);
    notifyCommandExecuted(m_commandExecutedCallback, result.commandAction,
                          result.matchedPhrase, result.confidence);
}

// TypingModeWorker

TypingModeWorker::TypingModeWorker(
    std::shared_ptr<CommandExecutor> executor,
    std::shared_ptr<SpeechPipeline> pipeline) {
    m_executor = executor;
    m_pipeline = pipeline;
    m_exitPhrases = {"stop typing", "exit typing", "normal mode", "go to normal mode"};
}

void TypingModeWorker::start() {
    m_isRunning = true;
    {
        std::lock_guard<std::mutex> lock(m_bufferMutex);
        m_buffer.clear();
    }
    m_pipeline->resetAsrStream();
    m_pipeline->typingWriter().reset();
    m_executor->log("INFO", "Typing mode worker started");
}

void TypingModeWorker::stop() {
    m_isRunning = false;
    {
        std::lock_guard<std::mutex> lock(m_bufferMutex);
        m_buffer.clear();
    }
    m_executor->log("INFO", "Typing mode worker stopped");
}

void TypingModeWorker::processTranscription(const TranscriptionResult& result) {
    if (!m_isRunning) return;

    {
        std::lock_guard<std::mutex> lock(m_bufferMutex);
        m_buffer = result.text;
    }

    if (checkExitPhrases(result.text)) {
        requestModeChange("normal");
        m_pipeline->speak("Normal mode", false, true);
        return;
    }

    if (result.isFinal || result.isEndpoint) {
        m_pipeline->typingWriter().processFinal(result.text);
        m_pipeline->resetAsrStream();
    } else if (m_pipeline->typingRealtime()) {
        m_pipeline->typingWriter().processPartial(result.text);
    }
}

void TypingModeWorker::processKeyword(const std::string& keyword) {
    if (!m_isRunning) return;
    if (keyword == "normal") {
        requestModeChange("normal");
        m_pipeline->speak("Normal mode", false, true);
    }
}

std::string TypingModeWorker::getBuffer() const {
    std::lock_guard<std::mutex> lock(m_bufferMutex);
    return m_buffer;
}

bool TypingModeWorker::checkExitPhrases(const std::string& text) const {
    const std::string norm = CommandIntentResolver::normalizeText(text);
    for (const auto& phrase : m_exitPhrases) {
        if (norm.find(CommandIntentResolver::normalizeText(phrase)) != std::string::npos) {
            return true;
        }
    }
    return false;
}

} // namespace VoiceAssistant
