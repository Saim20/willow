#pragma once

#include "CommandExecutor.hpp"
#include "CommandPhraseIndex.hpp"
#include <optional>
#include <string>
#include <vector>
#include <functional>
#include <chrono>
#include <mutex>

namespace VoiceAssistant {

struct CommandDispatchResult {
    bool handled{false};
    bool pending{false};
    bool blockedByPrefix{false};
    std::string matchedPhrase;
    std::string commandAction;
    std::string commandName;
    double confidence{0.0};
    bool isSearch{false};
    std::string searchEngine;
    std::string searchQuery;
    bool isSmartOpen{false};
    std::string appName;
};

class CommandIntentResolver {
public:
    using DispatchCallback = std::function<void(const CommandDispatchResult&)>;
    using PendingCallback = std::function<void(const std::string& phrase, bool blocked)>;

    CommandIntentResolver(std::shared_ptr<CommandExecutor> executor);

    void setCommands(const std::vector<Command>& commands);
    void setThreshold(double threshold) { m_threshold = threshold; }

    CommandDispatchResult processPartial(const std::string& text);
    CommandDispatchResult processEndpoint(const std::string& text);
    CommandDispatchResult processKeyword(const std::string& keyword);

    void setPendingCallback(PendingCallback callback) { m_pendingCallback = callback; }

    static std::string normalizeText(const std::string& text);
    static std::string normalizeSearchText(const std::string& text);

private:
    std::shared_ptr<CommandExecutor> m_executor;
    CommandPhraseIndex m_phraseIndex;
    std::vector<Command> m_commands;
    double m_threshold{0.8};
    PendingCallback m_pendingCallback;

    struct ExecutionRecord {
        std::string commandName;
        std::chrono::steady_clock::time_point timestamp;
    };
    std::vector<ExecutionRecord> m_executionHistory;
    std::mutex m_historyMutex;

    bool isDuplicate(const std::string& commandName);
    void recordExecution(const std::string& commandName);
    void cleanHistory();

    std::optional<std::pair<std::string, std::string>> parseSearch(const std::string& text) const;
    std::optional<std::string> parseSmartOpen(const std::string& text) const;
    CommandDispatchResult matchFuzzy(const std::string& text);
};

} // namespace VoiceAssistant
