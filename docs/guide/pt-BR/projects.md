---
id: projects
title: Projetos e etapas
group: production
place: projects
tour: projects
---

# Projetos e etapas

Um projeto de vídeo é um vídeo no caminho entre uma ideia aprovada e uma publicação. [Projetos](bardo:go/projects) mostra um projeto por vez: quem narra, as etapas do roteiro à publicação e quanto ele já custou. Aprovar uma ideia em [Temas](themes-ranking.md#review) começa um projeto. [Mostre a tela para mim](bardo:tour/projects).

<a id="switch"></a>
## Escolher um projeto

O nome do projeto abre a tela, com um menu ao lado que lista os outros projetos do canal; escolha um para trocar. O seletor de canal ao lado de **Projetos** lista os projetos de outro canal.

<a id="narrator"></a>
## O narrador

O narrador é a persona que lê a narração do vídeo, e o tom e o estilo de roteiro dela dão forma ao roteiro que o Claude escreve. Um projeto usa a persona padrão do canal até você escolher outra em **Narrador**, só para este vídeo. Mude o padrão do canal em [Canais](bardo:go/channels); crie e ajuste narradores em [Personas](personas.md).

<a id="stages"></a>
## As etapas

As etapas ficam no topo, em ordem: [Roteiro](script.md), [Narração](narration.md), [Cenas](scenes.md), [Clipes](clips.md), [Edição](editor.md), [Render](render.md) e Publicação. Escolha uma para abri-la; Edição abre o editor. A linha sob cada etapa diz como ela está:

- Quando está pronta, o que ela tem: as palavras do roteiro, a duração da narração, as imagens ou clipes feitos.
- **Gerando…** enquanto uma tarefa dela roda. Acompanhe em [Tarefas](bardo:go/jobs).
- **… para revisar** quando uma nova versão espera ao lado da atual.
- **Desatualizada** quando uma etapa anterior mudou depois que ela foi feita.
- **Depois do …** ou **Depois da …** quando ela está bloqueada (abaixo).

<a id="unlock"></a>
## Como uma etapa se abre

Cada etapa trabalha sobre o que a anterior lhe deu, então ela fica bloqueada até lá, e a linha dela diz o que espera:

- **Narração** se abre quando há um roteiro, porque o narrador o lê.
- **Cenas** se abre quando há uma narração, porque as cenas são cronometradas pelas palavras dela.
- **Clipes** se abre quando uma cena tem imagem, porque o clipe a anima.
- **Edição** se abre quando as cenas estão planejadas: o editor monta um primeiro corte a partir delas e da narração.
- **Render** e **Publicação** se abrem depois da edição e do render.

Nada é jogado fora quando uma etapa anterior muda. O que foi feito a partir dela aparece como **Desatualizada**, para você saber o que refazer: uma narração depois que o roteiro mudou, as cenas depois que a narração foi refeita.

<a id="cost"></a>
## Quanto já custou

A linha sob o nome do projeto mostra o nicho, quando ele começou e, assim que algo for pago, quanto o projeto já custou: cada chamada a um provedor para este vídeo, do roteiro aos clipes. Cada geração mostra a estimativa antes de você começar, e uma chamada que passaria de um orçamento pergunta antes. [Custos](bardo:go/costs) separa os gastos por provedor, canal e vídeo; veja [Suas chaves de API](api-keys.md#budgets) para os orçamentos.
