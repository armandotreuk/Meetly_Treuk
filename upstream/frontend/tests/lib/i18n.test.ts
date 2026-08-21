import { describe, expect, it } from "vitest";
import { t } from "@/lib/i18n";
import { en } from "@/lib/strings/en";

describe("t", () => {
    it("contains the expected i18n coverage", () => {
        expect(Object.keys(en).length).toBeGreaterThanOrEqual(51);
        expect(en["chat.closeAria"]).toBe("Close chat");
        expect(en["chat.sendAria"]).toBe("Send message");
        expect(en["app.recording.showNotesAria"]).toBe("Show recording notes");
        expect(en["app.meetingDetails.showNotesAria"]).toBe("Show notes");
        expect(en["app.meetingDetails.showChatAria"]).toBe("Show chat");
    });

    it("returns the English dictionary string", () => {
        expect(t("chat.header.title")).toBe(en["chat.header.title"]);
    });

    it("substitutes template variables", () => {
        expect(t("notes.status.savedAt", { time: "10:00" })).toBe("Saved 10:00");
    });

    it("returns missing keys", () => {
        expect(t("nonexistent.key")).toBe("nonexistent.key");
    });

    it("leaves templates unfilled without variables", () => {
        expect(t("notes.status.savedAt")).toBe("Saved {time}");
    });
});
