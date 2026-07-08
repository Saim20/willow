#include "CommandPhraseIndex.hpp"
#include <algorithm>
#include <cctype>

namespace VoiceAssistant {

std::string CommandPhraseIndex::normalize(const std::string& text) {
    std::string result = text;
    std::transform(result.begin(), result.end(), result.begin(),
                   [](unsigned char c) { return static_cast<char>(std::tolower(c)); });
    return result;
}

void CommandPhraseIndex::build(const std::vector<Command>& commands) {
    m_entries.clear();
    m_phraseIndex.clear();

    for (const auto& cmd : commands) {
        for (const auto& phrase : cmd.phrases) {
            PhraseEntry entry;
            entry.phrase = normalize(phrase);
            entry.commandName = cmd.name;
            entry.commandAction = cmd.command;
            if (!entry.phrase.empty()) {
                m_entries.push_back(entry);
                m_phraseIndex[entry.phrase].push_back(m_entries.size() - 1);
            }
        }
    }

    computePrefixConflicts();
}

void CommandPhraseIndex::computePrefixConflicts() {
    for (auto& entry : m_entries) {
        entry.hasPrefixExtension = false;
        for (const auto& other : m_entries) {
            if (other.phrase == entry.phrase) continue;
            if (other.phrase.size() > entry.phrase.size() &&
                other.phrase.compare(0, entry.phrase.size(), entry.phrase) == 0 &&
                (other.phrase[entry.phrase.size()] == ' ' ||
                 entry.phrase.empty())) {
                entry.hasPrefixExtension = true;
                break;
            }
        }
    }
}

bool CommandPhraseIndex::hasPrefixExtension(const std::string& phrase) const {
    const std::string norm = normalize(phrase);
    for (const auto& entry : m_entries) {
        if (entry.phrase == norm) {
            return entry.hasPrefixExtension;
        }
    }
    return false;
}

PhraseLookupResult CommandPhraseIndex::lookup(const std::string& text) const {
    PhraseLookupResult result;
    const std::string norm = normalize(text);

    for (const auto& entry : m_entries) {
        if (norm == entry.phrase) {
            result.exactMatch = true;
            result.matches.push_back(entry);
            if (entry.hasPrefixExtension) {
                result.blockedByPrefix = true;
            }
        }
    }

    return result;
}

} // namespace VoiceAssistant
