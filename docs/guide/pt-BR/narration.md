---
id: narration
title: Narração
group: production
place: projects/narration
tour: narration
---

# Narração

A narração é o roteiro lido em voz alta pelo narrador do projeto, com o tempo de cada palavra, para que cenas, legendas e cortes se alinhem a ela. Ela fica na etapa [Narração](bardo:go/projects/narration) de um projeto. [Mostre a etapa para mim](bardo:tour/narration).

<a id="generate"></a>
## Gerar a narração

**Gerar narração** faz a ElevenLabs ler o roteiro atual com a voz e os presets de geração do narrador ([Personas](personas.md#presets)). A ElevenLabs cobra por caractere; o ⓘ ao lado do botão diz quantos caracteres o roteiro tem, e a estimativa sob ele quanto isso custa. A geração roda como tarefa; é preciso uma chave da ElevenLabs ([Suas chaves de API](api-keys.md#providers)).

O botão espera até haver um roteiro e um narrador cuja voz esteja na sua conta da ElevenLabs. Se a voz do narrador nunca foi verificada ou não está lá, a etapa avisa; resolva em [Personas](personas.md#voice) ou escolha outro [narrador](projects.md#narrator).

<a id="play"></a>
## Ouvir a narração

**Tocar** toca a narração e destaca cada palavra enquanto é falada. Clique numa palavra para tocar a partir dela. **Detalhes** diz qual voz e modelo leram, quanto custou, a duração e quando foi feita.

<a id="stale"></a>
## Desatualizada

Quando o roteiro é salvo ou substituído depois que a narração foi feita, a narração fica marcada como **Desatualizada**: as palavras não batem mais. **Gere de novo**, ou importe uma nova gravação, para que batam. As cenas planejadas sobre uma narração anterior passam a aparecer como desatualizadas também ([Cenas](scenes.md#replan)).

<a id="import"></a>
## Importar uma gravação

Gravou o roteiro você mesmo? **Importar gravação** aceita um arquivo MP3 ou WAV sem compressão de até 500 MB. O Bardo mostra o nome, a duração e quanto custa marcar o tempo das palavras, e **Usar esta gravação** faz a ElevenLabs marcar o tempo de cada palavra conforme o roteiro atual, cobrado por hora de áudio. Ela substitui a narração atual quando o tempo das palavras fica pronto. Leia o roteiro como está escrito, para que o tempo bata.
