# ADR-0000: Record architecture decisions

## Status

Accepted

## Context

Проект находится на стадии, где почти всё ещё не решено: способ детекции, модель, платформа,
стек. Решения такого рода принимаются один раз, а объясняются потом многократно — людям и AI-
ассистентам. Без записи контекста и отвергнутых альтернатив каждое обсуждение начинается заново.

## Decision

Значимые архитектурные решения фиксируются как Architecture Decision Records в `docs/adr/`,
по одному файлу на решение, в формате `ADR-NNNN-slug.md`.

Структура записи: `Status` (Proposed / Accepted / Superseded), `Context`, `Decision`,
`Consequences`, при необходимости `Alternatives Considered`, `Related Documents`,
`Open Questions`.

ADR не редактируется задним числом после принятия: устаревшее решение помечается `Superseded`
и заменяется новым ADR.

## Consequences

- Новый участник или новая AI-сессия восстанавливает ход рассуждений из репозитория.
- Отвергнутые варианты не всплывают повторно.
- Каждое значимое решение стоит одного дополнительного файла.

## Related Documents

- `docs/product/vision.md`
- `docs/roadmap.md`
