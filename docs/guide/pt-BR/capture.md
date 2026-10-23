---
id: capture
title: Captura de atuação
group: production
place: projects/capture
tour: capture
---

# Captura de atuação

Uma captura é uma gravação sua lendo o roteiro do projeto diante da câmera. A fala vira a narração, na voz da persona ou na sua, e a gravação é dividida em tomadas que depois viram clipes interpretados. Ela fica na etapa [Captura](bardo:go/projects/capture), que aparece enquanto a narração é a da captura. [Mostre a etapa para mim](bardo:tour/capture).

<a id="record"></a>
## Gravar

Grave você lendo o roteiro atual como está escrito, sozinho na imagem, com o rosto iluminado e o microfone perto. O Bardo importa arquivos MP4 ou MOV de 4 segundos a 15 minutos, a 24 até 60 quadros por segundo e em 720p ou mais; um celular na vertical ou uma webcam bastam. Faça uma pausa curta entre as frases: as tomadas são cortadas em fins de frase e pausas.

Grave só você. O Bardo importa uma captura só depois que você confirma que a gravação mostra só você, e nunca importa a partir de um link.

<a id="import"></a>
## Importar

Na etapa [Narração](bardo:go/projects/narration), **Importar captura** abre a escolha de arquivo. O Bardo lê o arquivo primeiro e mostra tamanho, quadros por segundo e duração, ou por que ele não pode ser uma captura. Depois escolha a voz, marque **Esta gravação mostra só a mim.** e confira a estimativa: a conferência do roteiro, a voz e o tempo das palavras são cobrados por hora de som pela ElevenLabs. **Importar esta captura** roda como uma tarefa; é preciso uma chave da ElevenLabs ([Suas chaves de API](api-keys.md#providers)).

A captura substitui a narração atual quando termina. Gerar ou importar outra narração depois remove a captura.

<a id="voice"></a>
## A voz

**A voz da persona** converte a sua fala para a voz do narrador com os ajustes de voz dele (Voice Changer da ElevenLabs), mantendo o seu ritmo para que as palavras continuem nos seus lábios. A voz do narrador precisa estar na sua conta ElevenLabs ([Personas](personas.md#voice)). Uma voz realista recebe o rótulo de conteúdo sintético quando você publica ([Exportação](exporting.md)).

**Minha própria voz** mantém a sua voz, limpa de música e ruído do ambiente (Voice Isolator da ElevenLabs).

Só o som da gravação vai para a ElevenLabs; o vídeo fica na pasta do projeto.

<a id="check"></a>
## Conferir se segue o roteiro

Antes de pagar pela voz, a ElevenLabs transcreve o som e o Bardo confere se ele diz pelo menos 80% do roteiro na ordem. Uma gravação que não diz para com **Não segue o roteiro**, tendo pago só a transcrição. Grave de novo lendo o roteiro atual e importe o novo arquivo.

<a id="takes"></a>
## Tomadas

A captura é dividida em tomadas de 4 a 30 segundos. As tomadas começam e terminam em fins de frase, ou em pausas de 300 ms ou mais dentro de uma frase longa, e as frases são agrupadas até 30 segundos. Cada tomada mostra suas palavras, onde começa e termina na gravação e quanto dura.

Uma tomada que não pode virar clipe interpretado diz por quê: **Passa de 30 segundos** sem pausa para cortar (grave essa fala de novo com uma pausa), ou **Menos de 4 segundos** sem vizinha para juntar. A etapa pede atenção até não sobrar nenhuma.

<a id="cuts"></a>
## Mover um corte

**Cortar antes** e **Cortar depois** movem o corte entre uma tomada e a seguinte para o fim de frase ou a pausa anterior ou seguinte. As duas tomadas mudam, e o que foi feito do trecho antigo delas aparece como desatualizado.

<a id="stale"></a>
## Desatualizada

Quando o roteiro muda depois da captura, a narração dela fica desatualizada, e a captura também: grave de novo com o roteiro atual e importe.

<a id="remove"></a>
## Remover

**Remover captura** tira a gravação e suas tomadas do projeto. A narração feita com ela fica.
