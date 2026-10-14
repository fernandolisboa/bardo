---
id: metrics-sync
title: Sincronizar métricas
group: strategy
place: settings/metrics
tour: metrics-sync
---

# Sincronizar métricas

O Bardo lê os números dos seus posts em cada rede como uma tarefa: uma **sincronização**. [Configurações › Métricas](bardo:go/settings/metrics) diz quando ele faz isso sozinho. [Mostre a aba](bardo:tour/metrics-sync).

<a id="what"></a>
## O que uma sincronização lê

- **YouTube**: as visualizações, curtidas e comentários públicos de cada post vinculado, a uma unidade da cota diária a cada 50 posts: pela conta conectada do canal, quando ele tem uma, ou com a sua chave da YouTube Data API. Para um canal com a conta do YouTube conectada, também os números de dono do YouTube Analytics, a duas requisições por post.
- **Instagram** e **TikTok**: os números de cada post pela conta conectada do canal naquela rede, sem chave.
- **X** e **Kick**: nada; o Bardo guarda só o link.

Enquanto uma conta precisa reconectar, os números dos posts por essa conta ficam de fora; os números públicos do YouTube continuam sincronizando com a chave da Data API; veja [Solução de problemas](troubleshooting.md#reconnect).

<a id="on-start"></a>
## Sincronizar quando o Bardo abre

**Sincronizar ao abrir** decide se o Bardo sincroniza quando abre: nunca, ou quando a verificação mais antiga passou de 1 hora, 6 horas (a não ser que você mude), 12 horas ou um dia. Abrir o Bardo várias vezes no dia não gasta cota a mais. **Sincronizar agora**, em Desempenho, sempre funciona, seja qual for esta configuração.

Os números do YouTube são a exceção: quando um post do YouTube foi lido pela última vez há 25 dias ou mais, o Bardo sincroniza ao abrir mesmo com **Nunca**, porque números lidos do YouTube só podem ficar guardados 30 dias e, sem isso, seriam apagados ([Política de privacidade](privacy.md#kept)).

<a id="where"></a>
## Onde os números aparecem

Os números aparecem em [Desempenho](performance-metrics.md), por canal e por post, com a hora da última sincronização. A primeira semana de visualizações de cada post também alimenta o [ranqueamento de novas ideias](performance-metrics.md#ranking).
