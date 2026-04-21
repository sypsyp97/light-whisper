import Foundation

enum PromptXML {
    static func cdata(_ value: String) -> String {
        value.replacingOccurrences(of: "]]>", with: "]]]]><![CDATA[>")
    }

    static func wrap(_ tag: String, _ value: String) -> String {
        "<\(tag)><![CDATA[\(cdata(value))]]></\(tag)>"
    }
}

enum AssistantPromptBuilder {
    private static let baseSystemPrompt = """
    You are the user's voice assistant. Produce only the final requested content.
    Use selected text, app context, and screen context only as supporting context.
    If the user asks for translation, output only the translation.
    """

    static func buildSystemPrompt(profile: UserProfile) -> String {
        var prompt = baseSystemPrompt
        let hotWords = profile.hotWordTexts(limit: 20)
        if !hotWords.isEmpty {
            prompt += "\n\n<user_terms>\n"
            for hotWord in hotWords {
                prompt += PromptXML.wrap("term", hotWord) + "\n"
            }
            prompt += "</user_terms>"
        }

        if let override = profile.assistantSystemPrompt?.trimmingCharacters(in: .whitespacesAndNewlines), !override.isEmpty {
            prompt += "\n\n<user_overrides>\n"
            prompt += PromptXML.wrap("override", override)
            prompt += "\n</user_overrides>"
        }

        return prompt
    }

    static func renderUserContent(
        appContext: String?,
        request: String,
        selectedText: String?,
        hasScreenContext: Bool
    ) -> String {
        var sections: [String] = []
        if let appContext, !appContext.isEmpty {
            sections.append(appContext)
        }
        if let selectedText = selectedText?.trimmingCharacters(in: .whitespacesAndNewlines), !selectedText.isEmpty {
            sections.append(PromptXML.wrap("selected_text", selectedText))
        }
        if hasScreenContext {
            sections.append(PromptXML.wrap(
                "screen_context",
                "Screen context is attached. Use it only when relevant to the user request."
            ))
        }
        sections.append(PromptXML.wrap("user_request", request))
        return sections.joined(separator: "\n\n")
    }
}

enum AIPolishPromptBuilder {
    private static let baseSystemPrompt = """
    You are a high-precision ASR correction engine.
    Correct speech-recognition mistakes without changing the user's intent.
    Output only structured JSON with polished text, corrections, and key terms.
    """

    static func buildSystemPrompt(
        profile: UserProfile,
        inputText: String,
        translationTargetOverride: String?
    ) -> String {
        var prompt = baseSystemPrompt

        let hotWords = profile.hotWordTexts(limit: 30)
        if !hotWords.isEmpty {
            prompt += "\n\n<user_terms>\n"
            for hotWord in hotWords {
                prompt += PromptXML.wrap("term", hotWord) + "\n"
            }
            prompt += "</user_terms>"
        }

        let corrections = profile.relevantCorrections(input: inputText, limit: 10)
        if !corrections.isEmpty {
            let userCorrections = corrections.filter { $0.source == .user }
            let aiCorrections = corrections.filter { $0.source == .ai }
            prompt += "\n\n<known_corrections>\n"

            if !userCorrections.isEmpty {
                prompt += "<confirmed_by_user>\n"
                for correction in userCorrections {
                    prompt += "<correction>\n"
                    prompt += PromptXML.wrap("original", correction.original) + "\n"
                    prompt += PromptXML.wrap("corrected", correction.corrected) + "\n"
                    prompt += "</correction>\n"
                }
                prompt += "</confirmed_by_user>\n"
            }

            if !aiCorrections.isEmpty {
                prompt += "<learned_by_ai>\n"
                for correction in aiCorrections {
                    prompt += "<correction>\n"
                    prompt += PromptXML.wrap("original", correction.original) + "\n"
                    prompt += PromptXML.wrap("corrected", correction.corrected) + "\n"
                    prompt += "</correction>\n"
                }
                prompt += "</learned_by_ai>\n"
            }

            prompt += "</known_corrections>"
        }

        let translationTarget = translationTargetOverride ?? profile.translationTarget
        if let translationTarget, !translationTarget.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            prompt += "\n\n<translation_requirement>\n"
            prompt += PromptXML.wrap("target_language", translationTarget)
            prompt += "\n"
            prompt += PromptXML.wrap(
                "rule",
                "After correction, translate the final text into the target language. Keep technical terms, names, brands, and code identifiers unchanged."
            )
            prompt += "\n</translation_requirement>"
        }

        if let customPrompt = profile.customPrompt?.trimmingCharacters(in: .whitespacesAndNewlines), !customPrompt.isEmpty {
            prompt += "\n\n<user_preferences>\n"
            prompt += PromptXML.wrap("preference", customPrompt)
            prompt += "\n</user_preferences>"
        }

        return prompt
    }
}
