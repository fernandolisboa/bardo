---
id: themes-ranking
title: Temas e ranqueamento
group: strategy
place: themes
tour: themes
---

# Temas e ranqueamento

[Temas](bardo:go/themes) transforma um nicho em ideias de vídeo. O Claude escreve as ideias; o motor de decisão (TypeSafe) as ranqueia para o seu canal, com os motivos; você decide quais viram vídeo. Você precisa de uma chave do Claude para receber ideias e de uma chave da TypeSafe para ranqueá-las ([Suas chaves de API](api-keys.md#providers)). [Mostre a tela para mim](bardo:tour/themes).

<a id="pick"></a>
## Canal e nicho

As ideias são escritas para um canal e um nicho. Os nichos disponíveis são os da última pesquisa do canal em [Pesquisa](niche-research.md); antes de qualquer pesquisa, é o próprio nicho do canal. Quando o nicho tem resultados de pesquisa, a oportunidade, a concorrência e a tendência dele aparecem abaixo dos seletores, e uma linha diz como foram os vídeos publicados do canal, quando já têm números.

<a id="suggest"></a>
## Sugerir temas e ranquear de novo

**Sugerir temas** pede ao Claude 10 ideias de vídeo, cada uma com título e ângulo, escritas para o nicho, o público, o estilo e o idioma do canal. Depois, o motor de decisão ranqueia todas as ideias do nicho que ainda não têm ranqueamento. As duas coisas rodam como tarefa; o custo estimado aparece antes de você começar, e um orçamento definido em [Custos](bardo:go/costs) pergunta antes quando a execução passaria dele.

**Ranquear de novo** aparece quando há ideias esperando: as que você editou, as cujo ranqueamento falhou ou as ranqueadas antes de os seus vídeos terem números. Ele ranqueia só essas, sem pedir ideias novas.

<a id="ranking"></a>
## Prioridade e confiança

As ideias aparecem pela **prioridade**, de 0 a 100, da maior para a menor. A **confiança** é o quanto o motor de decisão teve certeza da sua resposta menos segura: uma prioridade alta com confiança baixa merece uma conferida sua.

<a id="reasons"></a>
## Como uma ideia é ranqueada

O motor dá nota a quatro motivos, cada um de 0 a 100 e com a sua própria confiança:

- **Encaixe**: o quanto a ideia combina com o canal, o nicho e o público dele.
- **Tendência**: quanta procura existe pela ideia agora.
- **Concorrência**: o quanto o ângulo está disputado; quanto menos, melhor.
- **Desempenho passado**: como ideias parecidas foram no seu canal, a partir das visualizações da primeira semana dos seus vídeos.

A prioridade pesa o encaixe em 40%, a tendência em 35% e o espaço que a concorrência deixa em 25%. O desempenho passado entra quando um vídeo publicado tem dois dias e os números dele foram sincronizados: ele vale 5% por vídeo assim, até 25% a partir de cinco vídeos, e os outros três dividem o resto nas mesmas proporções. **Detalhes** mostra os números que o motor leu e qual modelo ranqueou a ideia, e quando.

<a id="review"></a>
## Aprovar, editar ou descartar

- **Aprovar** começa um projeto de vídeo a partir da ideia, no canal. A ideia continua na lista, marcada como aprovada.
- **Editar** muda o título ou o ângulo. Uma ideia editada perde o ranqueamento, porque os motivos falavam do texto antigo; use **Ranquear de novo**.
- **Descartar** tira a ideia da lista. A lista conta quantas foram descartadas.

O Bardo não aprova nada no seu lugar: uma ideia só vira vídeo quando você a aprova.

<a id="projects"></a>
## Projetos começados aqui

A lista **Projetos**, abaixo dos controles, mostra os projetos começados a partir das ideias do canal. Abra-os em [Projetos](bardo:go/projects) para escrever o roteiro e depois a narração, as cenas e o resto ([Seu primeiro vídeo](first-video.md#project)).

<a id="next"></a>
## Próximo passo

Depois de publicar, vincule cada publicação na etapa Publicação do projeto. Os números dela aparecem então em [Desempenho](performance-metrics.md) e alimentam o desempenho passado dos próximos ranqueamentos.
