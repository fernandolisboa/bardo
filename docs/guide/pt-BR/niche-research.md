---
id: niche-research
title: Pesquisa de nichos
group: strategy
place: research
tour: research
---

# Pesquisa de nichos

A [Pesquisa](bardo:go/research) mostra o quanto um nicho está concorrido no YouTube e a rapidez com que os vídeos recentes dele ganham visualizações, para você escolher nichos com espaço. Ela lê dados públicos do YouTube com a sua chave da YouTube Data API ([Suas chaves de API](api-keys.md#providers)). [Mostre a tela para mim](bardo:tour/research).

<a id="market"></a>
## Canal e mercado

A pesquisa roda para um canal por vez. O **país-alvo** e o **idioma do conteúdo** do canal definem o mercado: cada busca pede ao YouTube os uploads naquele país e idioma, então o mesmo nicho pode ter notas diferentes para um canal em inglês para os EUA e outro em português para o Brasil. Mude esses dados em [Canais](bardo:go/channels); os resultados de cada mercado ficam separados.

<a id="seeds"></a>
## Nichos ou palavras-chave

Digite um nicho ou palavra-chave por linha, até 20 por pesquisa e 100 caracteres cada. Cada linha vira uma busca no YouTube, então escreva do jeito que o público busca ("mistérios da guerra fria", e não "conteúdo de história"). Linhas que só mudam em maiúsculas ou espaços contam uma vez. A lista da última pesquisa fica guardada com o canal para a próxima vez.

<a id="run"></a>
## Pesquisar, atualizar e cota

**Pesquisar** começa uma tarefa que consulta os nichos. Cada busca custa cerca de 1% da cota diária padrão da YouTube Data API (10.000 unidades), e o ⓘ ao lado dos botões diz quanto a pesquisa vai custar antes de você começar.

Cada resultado fica guardado por **7 dias** por nicho, país e idioma. Pesquisar de novo dentro de uma semana reaproveita o resultado e não gasta cota; um resultado mais velho que isso avisa no próprio cartão e é buscado de novo na próxima pesquisa. **Atualizar tudo** busca todos os nichos de novo agora, seja qual for a idade, e mostra o custo no próprio botão.

A tarefa roda em segundo plano: continue trabalhando, e acompanhe ou cancele em [Tarefas](bardo:go/jobs).

<a id="results"></a>
## Como ler os resultados

Os nichos aparecem pela **oportunidade**, da maior para a menor. Cada cartão mostra três notas de 0 a 100 e os números por trás delas, todos dos uploads dos últimos **30 dias** no mercado:

- **Uploads, 30 dias**: quantos vídeos o YouTube estima que foram postados para a busca.
- **Views (mediana)** e **views por dia**: o quanto os uploads recentes mais relevantes (até 50) foram vistos, e com que rapidez.
- **Inscritos (mediana)** e **canais pequenos**: o tamanho dos canais por trás desses uploads, e quantos têm menos de 10.000 inscritos.
- **Buscado**: quando os números foram lidos. Um resultado com mais de 7 dias avisa; atualize para ver números atuais.

Um nicho sem uploads no período fica sem notas, porque não há nada para avaliá-lo.

<a id="scores"></a>
## Como as notas são calculadas

Cada número passa por uma escala que cresce por ordens de grandeza (ir de 1.000 para 10.000 uploads conta tanto quanto ir de 10.000 para 100.000), para que nichos enormes não achatem os demais.

- **Concorrência** (quanto maior, mais difícil): 40% vêm de quantos uploads existem, 40% do tamanho dos canais que postam e de quão poucos canais pequenos aparecem, e 20% de quão poucas visualizações cada vídeo recebe.
- **Tendência** (quanto maior, mais em alta): quantas visualizações por dia os uploads recentes ganham.
- **Oportunidade**: a média entre a tendência e o espaço que a concorrência deixa (100 menos a concorrência).

As notas comparam nichos do mesmo mercado. Elas não preveem visualizações; os números dos seus próprios vídeos fazem isso com o tempo, em [Desempenho](performance-metrics.md).

<a id="next"></a>
## Próximo passo

Leve os nichos com melhor oportunidade para [Temas](bardo:go/themes): o Claude sugere ideias de vídeo para um deles e o motor de decisão as ranqueia para o seu canal. Veja [Temas e ranqueamento](themes-ranking.md).
