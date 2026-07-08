#include "CommandIntentResolver.hpp"
#include <algorithm>
#include <regex>

namespace VoiceAssistant {

CommandIntentResolver::CommandIntentResolver(std::shared_ptr<CommandExecutor> executor)
    : m_executor(std::move(executor)) {}

void CommandIntentResolver::setCommands(const std::vector<Command>& commands) {
    m_commands = commands;
    m_phraseIndex.build(commands);
}

std::string CommandIntentResolver::normalizeText(const std::string& text) {
    std::string result = text;
    std::transform(result.begin(), result.end(), result.begin(),
                   [](unsigned char c) { return static_cast<char>(std::tolower(c)); });
    std::regex multiSpace(R"(\s+)");
    result = std::regex_replace(result, multiSpace, " ");
    result.erase(0, result.find_first_not_of(" \t"));
    if (!result.empty()) {
        result.erase(result.find_last_not_of(" \t") + 1);
    }
    return result;
}

std::string CommandIntentResolver::normalizeSearchText(const std::string& text) {
    std::string result = normalizeText(text);
    std::regex fourWord(R"(\bfour\b)");
    result = std::regex_replace(result, fourWord, "for");
    return result;
}

bool CommandIntentResolver::isDuplicate(const std::string& commandName) {
    cleanHistory();
    std::lock_guard<std::mutex> lock(m_historyMutex);
    const auto now = std::chrono::steady_clock::now();
    for (const auto& record : m_executionHistory) {
        if (record.commandName == commandName) {
            const auto elapsed = std::chrono::duration_cast<std::chrono::milliseconds>(
                now - record.timestamp).count();
            if (elapsed < 2000) return true;
        }
    }
    return false;
}

void CommandIntentResolver::recordExecution(const std::string& commandName) {
    std::lock_guard<std::mutex> lock(m_historyMutex);
    m_executionHistory.push_back({commandName, std::chrono::steady_clock::now()});
}

void CommandIntentResolver::cleanHistory() {
    std::lock_guard<std::mutex> lock(m_historyMutex);
    const auto now = std::chrono::steady_clock::now();
    m_executionHistory.erase(
        std::remove_if(m_executionHistory.begin(), m_executionHistory.end(),
            [&now](const ExecutionRecord& r) {
                return std::chrono::duration_cast<std::chrono::seconds>(now - r.timestamp).count() > 5;
            }),
        m_executionHistory.end());
}

std::optional<std::pair<std::string, std::string>> CommandIntentResolver::parseSearch(
    const std::string& text) const {
    const std::string norm = normalizeSearchText(text);

    // Pattern: "search [engine] for [query]"
    const std::regex patternFor(R"(search\s+(\w+)\s+for\s+(.+))");
    std::smatch match;
    if (std::regex_search(norm, match, patternFor) && match.size() >= 3) {
        return std::make_pair(match[1].str(), match[2].str());
    }

    // Pattern: "search [engine] [query]"
    const std::regex patternShort(R"(search\s+(\w+)\s+(.+))");
    if (std::regex_search(norm, match, patternShort) && match.size() >= 3) {
        const std::string engine = match[1].str();
        const std::string query = match[2].str();
        if (query != "for") {
            return std::make_pair(engine, query);
        }
    }

    // Pattern: "[engine] [query]" for known engines
    const auto& engines = m_executor->getContextConfig().searchEngines;
    for (const auto& [engine, url] : engines) {
        (void)url;
        if (norm.size() > engine.size() + 1 &&
            norm.compare(0, engine.size(), engine) == 0 &&
            norm[engine.size()] == ' ') {
            return std::make_pair(engine, norm.substr(engine.size() + 1));
        }
    }

    return std::nullopt;
}

std::optional<std::string> CommandIntentResolver::parseSmartOpen(const std::string& text) const {
    const std::string norm = normalizeText(text);
    const std::vector<std::string> triggers = {"open ", "launch ", "start "};
    for (const auto& trigger : triggers) {
        const size_t pos = norm.find(trigger);
        if (pos != std::string::npos) {
            std::string appName = norm.substr(pos + trigger.size());
            appName.erase(0, appName.find_first_not_of(" \t"));
            appName.erase(appName.find_last_not_of(" \t") + 1);
            if (!appName.empty()) return appName;
        }
    }
    return std::nullopt;
}

CommandDispatchResult CommandIntentResolver::processKeyword(const std::string& keyword) {
    CommandDispatchResult result;
    const std::string norm = normalizeText(keyword);
    const auto lookup = m_phraseIndex.lookup(norm);

    if (lookup.exactMatch && !lookup.blockedByPrefix && !lookup.matches.empty()) {
        result.handled = true;
        result.matchedPhrase = lookup.matches[0].phrase;
        result.commandName = lookup.matches[0].commandName;
        result.commandAction = lookup.matches[0].commandAction;
        result.confidence = 1.0;
    }
    return result;
}

CommandDispatchResult CommandIntentResolver::processPartial(const std::string& text) {
    CommandDispatchResult result;
    const std::string norm = normalizeText(text);

    if (norm.find("search") == 0) {
        result.pending = true;
        return result;
    }

    const auto lookup = m_phraseIndex.lookup(norm);
    if (lookup.exactMatch) {
        result.pending = true;
        result.blockedByPrefix = lookup.blockedByPrefix;
        if (!lookup.matches.empty()) {
            result.matchedPhrase = lookup.matches[0].phrase;
            result.commandName = lookup.matches[0].commandName;
            result.commandAction = lookup.matches[0].commandAction;
        }
        if (m_pendingCallback) {
            m_pendingCallback(result.matchedPhrase, result.blockedByPrefix);
        }
    }

    return result;
}

CommandDispatchResult CommandIntentResolver::processEndpoint(const std::string& text) {
    const std::string norm = normalizeText(text);

    // Search - endpoint only
    if (auto search = parseSearch(text)) {
        CommandDispatchResult result;
        result.handled = true;
        result.isSearch = true;
        result.searchEngine = search->first;
        result.searchQuery = search->second;
        result.matchedPhrase = "search " + search->first + " for " + search->second;
        result.confidence = 1.0;
        const std::string key = "smart_search_" + search->first + "_" + search->second;
        if (!isDuplicate(key)) {
            recordExecution(key);
        } else {
            result.handled = false;
        }
        return result;
    }

    // Smart open
    if (auto app = parseSmartOpen(text)) {
        CommandDispatchResult result;
        const std::string key = "smart_open_" + *app;
        if (isDuplicate(key)) {
            result.handled = true;
            return result;
        }
        result.handled = true;
        result.isSmartOpen = true;
        result.appName = *app;
        result.matchedPhrase = "open " + *app;
        result.confidence = 1.0;
        recordExecution(key);
        return result;
    }

    // Exact phrase match
    const auto lookup = m_phraseIndex.lookup(norm);
    if (lookup.exactMatch && !lookup.blockedByPrefix && !lookup.matches.empty()) {
        CommandDispatchResult result;
        const auto& match = lookup.matches[0];
        if (isDuplicate(match.commandName)) {
            result.handled = true;
            return result;
        }
        result.handled = true;
        result.matchedPhrase = match.phrase;
        result.commandName = match.commandName;
        result.commandAction = match.commandAction;
        result.confidence = 1.0;
        recordExecution(match.commandName);
        return result;
    }

    // Fuzzy fallback
    return matchFuzzy(norm);
}

CommandDispatchResult CommandIntentResolver::matchFuzzy(const std::string& text) {
    CommandDispatchResult result;
    auto [bestCmd, confidence] = m_executor->findBestMatch(text, m_commands, m_threshold);
    if (bestCmd && confidence >= m_threshold) {
        if (isDuplicate(bestCmd->name)) {
            result.handled = true;
            return result;
        }
        result.handled = true;
        result.commandName = bestCmd->name;
        result.commandAction = bestCmd->command;
        result.confidence = confidence;
        for (const auto& phrase : bestCmd->phrases) {
            if (m_executor->matchPhrase(text, phrase) >= m_threshold) {
                result.matchedPhrase = phrase;
                break;
            }
        }
        if (result.matchedPhrase.empty() && !bestCmd->phrases.empty()) {
            result.matchedPhrase = bestCmd->phrases.front();
        }
        recordExecution(bestCmd->name);
    }
    return result;
}

} // namespace VoiceAssistant
