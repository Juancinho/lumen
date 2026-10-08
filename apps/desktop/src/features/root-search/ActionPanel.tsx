import type { ActionView } from "../../ipc";
import { ACTION_LIST_ID, actionDomId } from "./model";

interface ActionPanelProps {
  /** Title of the result the actions apply to. */
  subject: string;
  actions: readonly ActionView[];
  selectedIndex: number;
  onSelect: (index: number) => void;
  onRun: (index: number) => void;
}

/**
 * The Action Panel (T108, DESIGN_SYSTEM "Action Panel"): a continuation of the surface
 * anchored bottom-right over the list, primary action first, destructive ones separated.
 * Focus stays in the query field, which drives it with the keyboard.
 */
export function ActionPanel({
  subject,
  actions,
  selectedIndex,
  onSelect,
  onRun,
}: ActionPanelProps) {
  return (
    <section className="action-panel" aria-label={`Actions for ${subject}`}>
      <header className="action-panel__subject" title={subject}>
        {subject}
      </header>
      <div id={ACTION_LIST_ID} role="listbox" aria-label="Actions" className="action-panel__list">
        {actions.map((action, index) => (
          // Keyboard is owned by the query field (aria-activedescendant).
          // eslint-disable-next-line jsx-a11y/click-events-have-key-events
          <div
            key={action.id}
            id={actionDomId(index)}
            role="option"
            aria-selected={index === selectedIndex}
            tabIndex={-1}
            className="action-panel__item"
            data-group={action.group}
            onMouseMove={() => {
              if (index !== selectedIndex) onSelect(index);
            }}
            onMouseDown={(event) => {
              event.preventDefault();
            }}
            onClick={() => {
              onRun(index);
            }}
          >
            <span className="action-panel__title">{action.title}</span>
            {action.shortcut && (
              <kbd className="action-panel__key" aria-hidden="true">
                {action.shortcut.replace("Enter", "↵").replace("+", " ")}
              </kbd>
            )}
          </div>
        ))}
      </div>
    </section>
  );
}
