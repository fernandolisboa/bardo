---
id: performance-metrics
title: Desempenho e métricas
group: strategy
place: performance
tour: performance
---

# Desempenho e métricas

O [Desempenho](bardo:go/performance) acompanha como se saem as publicações de um canal: visualizações, curtidas, comentários e, quando a rede informa, tempo de exibição, retenção e receita. Esses números também ensinam ao [ranqueamento de temas](themes-ranking.md#reasons) o que funciona no seu canal. [Mostre a tela para mim](bardo:tour/performance).

<a id="channel"></a>
## Um canal por vez

Escolha o canal no cabeçalho. Os números dele somam os dados mais recentes de todas as publicações vinculadas: visualizações, curtidas e comentários (e compartilhamentos, quando o Instagram ou o TikTok informam), além da quantidade de publicações. Quando a conta do YouTube do canal está conectada em [Contas](bardo:go/accounts), **visualizações engajadas**, **tempo de exibição** e **receita estimada** passam à frente. O gráfico das visualizações do canal ao longo das sincronizações começa depois de duas.

<a id="posts"></a>
## Publicações vinculadas

A lista tem cada publicação vinculada a um projeto do canal, da mais nova para a mais antiga, com a rede e as visualizações. Uma publicação marcada como **Não encontrado** não apareceu na última sincronização (foi removida ou ficou privada); os números dela ficam como estavam. Uma publicação que o Bardo enviou também mostra como foi o envio (ainda em processamento, enviada ao TikTok como rascunho, interrompida), e o ⓘ dela diz o que fazer a seguir.

<a id="numbers"></a>
## Os números de uma publicação

Escolha uma publicação para abri-la ao lado da lista: o link, os números, quanto cada um mudou desde a sincronização anterior e o histórico ao longo das sincronizações. A origem depende da rede:

- **YouTube**: visualizações, curtidas e comentários públicos, lidos com a sua chave da YouTube Data API. Com a conta do YouTube do canal conectada, o YouTube Analytics acrescenta visualizações engajadas (as que passam dos primeiros segundos), tempo de exibição, duração média, retenção do público e receita estimada com RPM e CPM. Esses dados chegam com 2 a 3 dias de atraso.
- **Instagram** e **TikTok**: os números da publicação, depois que a conta do canal nessa rede está conectada; sem ela, não há contagem pública para ler.
- **X** e **Kick**: o Bardo guarda o link e não lê números.

<a id="sync"></a>
## Sincronização

**Sincronizar agora** lê os números mais recentes de cada publicação vinculada, como tarefa, e a linha ao lado diz quando foi a última sincronização. Os números públicos do YouTube custam uma unidade de cota a cada 50 publicações. O Bardo também sincroniza sozinho quando abre, se a última verificação passou do tempo definido em [Configurações › Métricas](bardo:go/settings/metrics) (6 horas, a não ser que você mude, ou nunca).

<a id="link"></a>
## Vincular e desvincular uma publicação

As publicações são vinculadas na etapa [Publicação](bardo:go/projects/publish) de um projeto. Um vídeo que o Bardo envia se vincula sozinho. Para um arquivo exportado, poste à mão e depois cole o link da publicação em **Marcar como publicado**; o Bardo confere se o link é de uma publicação naquela rede. **Desvincular** tira a publicação e os números dela do Bardo; a publicação continua na rede.

<a id="ranking"></a>
## Como os seus números ranqueiam ideias

As visualizações de cada publicação sete dias depois de ir ao ar, a **primeira semana**, são o que o ranqueamento de temas compara. O Bardo lê esse número das sincronizações em torno do sétimo dia, ou projeta a partir dos primeiros dias. Quando um vídeo publicado tem dois dias e foi sincronizado, o desempenho passado entra no ranqueamento das novas ideias em [Temas](themes-ranking.md#reasons), e as ideias ranqueadas antes podem ser ranqueadas de novo para incluí-lo.
