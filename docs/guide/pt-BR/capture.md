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

Grave-se lendo o roteiro atual como está escrito, sozinho na imagem, com o rosto iluminado e o microfone perto. O Bardo importa arquivos MP4 ou MOV de 4 segundos a 15 minutos, de 24 a 60 quadros por segundo e em 720p ou mais; um celular na vertical ou uma webcam bastam. Faça uma pausa curta entre as frases: as tomadas são cortadas em fins de frase e pausas.

Grave só você. O Bardo importa uma captura só depois que você confirma que a gravação mostra só você, e nunca importa a partir de um link.

<a id="import"></a>
## Importar

Na etapa [Narração](bardo:go/projects/narration), **Importar captura** abre a escolha de arquivo. O Bardo lê o arquivo primeiro e mostra tamanho, quadros por segundo e duração, ou por que ele não pode ser uma captura. Depois escolha a voz, marque **Só eu apareço nesta gravação.** e confira a estimativa: a conferência do roteiro, a voz e o tempo das palavras são cobrados por hora de som pela ElevenLabs. **Importar esta captura** roda como uma tarefa; é preciso uma chave da ElevenLabs ([Suas chaves de API](api-keys.md#providers)).

A captura substitui a narração atual quando termina. Gerar ou importar outra narração depois remove a captura.

<a id="voice"></a>
## A voz

**A voz da persona** converte a sua fala para a voz do narrador com os ajustes de voz dele (Voice Changer da ElevenLabs), mantendo o seu ritmo para que as palavras continuem nos seus lábios. A voz do narrador precisa estar na sua conta ElevenLabs ([Personas](personas.md#voice)). Uma voz realista recebe o rótulo de conteúdo sintético quando você publica ([Exportação](exporting.md)).

**Minha própria voz** mantém a sua voz, limpa de música e ruído do ambiente (Voice Isolator da ElevenLabs).

Só o som da gravação vai para a ElevenLabs; o vídeo fica na pasta do projeto.

<a id="check"></a>
## Conferir se segue o roteiro

Antes de pagar pela voz, a ElevenLabs transcreve o som e o Bardo confere se ele diz pelo menos 80% do roteiro na ordem, e se pelo menos 80% do que ele diz é o roteiro. Uma gravação que não diz para dizendo que a gravação não segue o roteiro de perto o bastante, tendo pago só a transcrição. Grave de novo lendo o roteiro atual e importe o novo arquivo.

<a id="takes"></a>
## Tomadas

A captura é dividida em tomadas de 4 a 30 segundos. As tomadas começam e terminam em fins de frase, ou em pausas de 300 ms ou mais dentro de uma frase longa, e as frases são agrupadas até 30 segundos. Cada tomada mostra suas palavras, onde começa e termina na gravação e quanto dura.

Uma tomada que não pode virar clipe interpretado diz por quê: **Passa de 30 segundos** sem pausa para cortar (grave essa fala de novo com uma pausa), ou **Menos de 4 segundos** sem vizinha para juntar. A etapa pede atenção até não sobrar nenhuma.

<a id="mode"></a>
## O modo da tomada

Cada tomada tem um modo: o que o modelo de tomada recebe. **Gravação** envia a tomada como foi gravada. **Malha** cobre seu rosto com a malha desenhada. **Profundidade** mostra você como uma figura de profundidade sobre o cenário, e também precisa da profundidade e da máscara da pessoa da tomada. As tomadas novas começam em Gravação; escolha outro modo abaixo das palavras da tomada.

<a id="analysis"></a>
## A análise

O Bardo analisa cada tomada neste computador, de graça, e nada sai dele. Assim que as tomadas existem, ele encontra seu rosto em cada quadro e guarda a malha: 478 pontos e 52 notas de expressão por quadro, no `face_landmarks.json` da tomada, na pasta do projeto. Passar uma tomada para **Profundidade** também calcula a profundidade e a máscara da pessoa, sobre a tomada inteira de uma vez para a profundidade não pulsar de um quadro para outro; voltar a outro modo as interrompe.

Cada parte é um job por tomada, mostrado abaixo da tomada com o progresso, onde roda e quanto falta, mais ou menos. Roda na placa de vídeo quando pode; num computador sem uma utilizável, o processador roda, o que demora mais, mas funciona. Cancelar guarda o que já terminou: **Retomar** continua do último trecho de quadros terminado. Mover um corte analisa as duas tomadas de novo.

<a id="checks"></a>
## As checagens

Quando a análise de uma tomada termina, ela diz o que encontrou, em quadros:

- **Mais de um rosto**: só você pode aparecer na gravação. A tomada não pode seguir; grave-a de novo sozinho.
- **Nenhum rosto**: quadros em que seu rosto sai da imagem ou fica escondido.
- **Rosto pequeno demais**: menos de 15% da altura do quadro. Chegue mais perto da câmera.
- **Cabeça virada**: mais de 45° de lado em relação à câmera.
- **Saltos da máscara** (modo Profundidade): a máscara da pessoa mudando mais de 20% de um quadro para o seguinte.

<a id="cuts"></a>
## Mover um corte

**Cortar antes** e **Cortar depois** movem o corte entre uma tomada e a seguinte para o fim de frase ou a pausa mais próxima, antes ou depois, que deixe as duas tomadas com 4 a 30 segundos. As duas tomadas mudam, e o que foi feito do trecho antigo delas aparece como desatualizado.

<a id="stale"></a>
## Desatualizada

Quando o roteiro muda depois da captura, a narração dela fica desatualizada, e a captura também: grave de novo com o roteiro atual e importe.

<a id="remove"></a>
## Remover

**Remover captura** tira a gravação e suas tomadas do projeto. A narração feita com ela fica. O Bardo pergunta uma vez antes; para ter as tomadas de volta, importe a gravação de novo.
