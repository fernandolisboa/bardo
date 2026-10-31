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

Cada tomada tem um modo: o que o modelo de tomada recebe. **Gravação** envia a tomada como foi gravada. **Malha** cobre seu rosto com a malha desenhada. **Profundidade** mostra você como uma figura de profundidade sobre o cenário, e também precisa da profundidade e da máscara da pessoa da tomada. As tomadas novas começam no primeiro modo que o modelo de tomada delas aceita ([Clipe interpretado](#performed)): Gravação no modelo padrão. Escolha outro abaixo das palavras da tomada; um modo que o modelo da tomada não aceita fica apagado.

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

<a id="control"></a>
## O vídeo de controle

Cada tomada tem um vídeo de controle: exatamente o que o modelo de tomada vai receber, montado neste computador, de graça, assim que a análise da tomada termina. É a tomada no modo dela, com o trecho da narração como som, num MP4 que serve para todo modelo de tomada: no máximo 1920 × 1080 (ou 1080 × 1920 em pé), nunca maior que a gravação, e bem abaixo de 100 MB. Gravação mantém a taxa de quadros da gravação; Malha e Profundidade são desenhadas em cada quadro analisado.

- **Gravação** é o trecho da gravação como está.
- **Malha** cobre seu rosto, quadro a quadro, com um oval preenchido num cinza-ardósia escuro e a malha desenhada por cima: lábios, olhos e sobrancelhas cada um na sua cor. Um quadro em que não se achou rosto usa os rostos dos quadros mais próximos dentro de meio segundo, com a cobertura maior quanto mais longe estiverem, caso a cabeça tenha se mexido. Uma lacuna maior mostraria o seu rosto: a revisão conta esses quadros, e a tomada não pode ser aprovada até você gravá-la de novo ou passá-la para Profundidade.
- **Profundidade** desenha você como uma figura colorida pela profundidade, em cores quentes o que está perto e frias o que está longe, dentro da máscara da pessoa, com as linhas da malha sobre o rosto, sobre o [cenário](#set).

Mudar o modo ou o cenário da tomada, ou mover um corte, monta de novo. **Montar de novo** inicia um que falhou ou foi interrompido.

<a id="review"></a>
## Revisar e aprovar

Abaixo da tomada, **Reproduzir** toca o vídeo de controle com o som, e uma linha diz o que sai do computador com ele: seu rosto, corpo e ambiente (Gravação); seu corpo e ambiente, não o rosto (Malha); ou sua silhueta e a geometria do rosto como figura de profundidade sobre o seu ambiente ou sobre a imagem de cenário (Profundidade). A voz da tomada sempre vai junto. A não ser que a tomada esteja em Profundidade sobre uma imagem, um quadro do vídeo de controle também vai para o Claude descrever o seu ambiente quando o [prompt](#take-prompt) dela for escrito; em Gravação, esse quadro mostra você.

**Aprovar** diz que esta tomada pode ir para o modelo de tomada; nada é pago por uma tomada antes da sua aprovação. A aprovação vale para o que você assistiu: mudar o modo, o cenário ou o corte da tomada, ou a análise dela rodar de novo, pede que você assista e aprove a nova montagem. Uma tomada com mais de um rosto, uma tomada em Malha com quadros descobertos ou uma tomada com uma duração que nenhum modelo de tomada aceita não pode ser aprovada.

<a id="set"></a>
## O cenário

No modo Profundidade, **Cenário** escolhe onde você aparece: **Seu ambiente**, como gravado; uma **Imagem de cena** do projeto; ou **Importar imagem**, um PNG ou JPEG seu. A imagem é copiada para a pasta do projeto, então a tomada a mantém aconteça o que acontecer com a cena ou o arquivo. Sobre uma imagem, a imagem da gravação nunca é decodificada, e nenhum pixel dela sai do computador.

<a id="performed"></a>
## Clipe interpretado

Depois que o vídeo de controle de uma tomada é aprovado, o modelo de tomada faz a persona interpretá-la: a [aparência](personas.md#look) da persona se mexe, fala e vira como você na tomada. As tomadas vão para o modelo de tomada do canal ([Canais](channels.md#defaults)); **Modelo de tomada**, abaixo da tomada, escolhe outro só para ela. Cada modelo aparece com o provedor em que roda, com a sua chave de lá ([Suas chaves de API](api-keys.md#providers)):

- **Kling 3.0 Motion Control · Higgsfield**, o padrão, aceita tomadas em Gravação de 3 a 30 segundos e faz a versão final direto, em 1080p.
- **Seedance 2.5 Video Edit · Higgsfield** aceita tomadas em Malha e Profundidade de 4 a 30 segundos: primeiro um rascunho em 480p, depois a versão final em 1080p, feita do zero.
- **Seedance 2.5 Video Edit · BytePlus ModelArk** aceita as mesmas tomadas, direto do ModelArk: um rascunho em 480p, depois uma versão final que completa esse mesmo rascunho em 1080p, então a versão final mantém o que você aprovou no rascunho. Veja [No BytePlus ModelArk](#byteplus).
- **Kling 3.0 Motion Control 720p · Kling** e **Kling 3.0 Motion Control 1080p · Kling** aceitam tomadas em Gravação de 3 a 30 segundos direto do Kling, com a chave do Kling que o Bardo já guarda para clipes, por menos do que pela Higgsfield. Um rascunho opcional no Kling 2.6 em 720p custa cerca de metade de uma versão final em 720p. Veja [Direto do Kling](#kling).

Escolher um modelo que não aceita o modo da tomada passa a tomada para o primeiro modo do modelo, o que monta o vídeo de controle de novo para você aprovar. Nada pode ser feito enquanto a tomada for curta ou longa demais para o modelo, a persona não tiver aparência ou o vídeo de controle não estiver aprovado; a tomada diz qual.

<a id="spend"></a>
## Quanto as tomadas do vídeo custam

**Tomadas interpretadas deste vídeo**, acima das tomadas, soma quanto as tomadas interpretadas do vídeo já custaram (prompts, o teste, rascunhos e versões finais, como [Custos](costs.md) registra) e quanto ainda falta, aproximadamente: o prompt de cada tomada que não tem um, um rascunho quando o modelo exige e a versão final de cada tomada que não tem uma. Isso aparece comparado ao teto do vídeo, US$ 10, a não ser que você mude em **Mudar teto** (no mínimo US$ 1; **Usar o padrão** volta para US$ 10). Quando uma etapa não tem preço na tabela, a linha avisa que o total está incompleto.

Antes de qualquer tomada paga (teste, rascunho ou versão final), o Bardo confere se a projeção mais essa tomada fica dentro do teto; uma tomada que já está na projeção não conta duas vezes. Acima do teto, a tomada espera e pergunta **Fazer mesmo assim?**: nada é enviado até você confirmar. O teto é por vídeo e não substitui os orçamentos mensais dos provedores ([Custos](costs.md)): quando uma tomada passa dos dois, uma única pergunta lista o teto e cada orçamento que ela ultrapassa, e confirmar envia a tomada.

<a id="test"></a>
## A tomada de teste

Antes do primeiro rascunho ou versão final de uma captura, o Bardo pede um teste: **Fazer tomada de teste** envia até os primeiros 5 segundos de uma tomada pronta (vídeo de controle aprovado e prompt), a primeira que passou nas [checagens antes do envio](#preflight) quando houver uma, na opção mais barata do modelo (a de rascunho, quando ele tem uma). Ela mostra quanto custa antes de começar, é registrada em [Custos](costs.md) como qualquer tomada paga e toca com a voz da tomada. Depois:

- **Aprovar teste** libera os rascunhos e versões finais da captura.
- **Descartar teste** o apaga; **Fazer tomada de teste** de novo faz outro, que o substitui.

Um teste vale para a aparência da persona com que foi feito: outra aparência pede um novo. Uma captura que já tinha rascunho ou versão final quando os testes chegaram não precisa de um.

<a id="preflight"></a>
## Checagens antes do envio

Abaixo de cada tomada, **Checagens antes do envio** leem a análise da tomada e a aparência, de graça, antes de pagar qualquer coisa:

- **Uma pessoa**: nenhum quadro com outro rosto (um já bloqueia o vídeo de controle).
- **Dentro do quadro**: em no máximo 10% dos quadros o rosto falta, está numa borda ou sem espaço para o corpo abaixo dele.
- **Rosto grande o suficiente**: o rosto tem pelo menos 8% da altura do quadro, em média.
- **Movimento estável**: em no máximo 5% dos quadros o rosto se move mais rápido que duas alturas de rosto por segundo.
- **Mesmo enquadramento da aparência**: uma tomada da cintura para cima pede uma aparência enquadrada igual, e uma tomada de corpo inteiro, uma aparência de corpo inteiro. Modelos de controle de movimento seguem o enquadramento da aparência; modelos de edição de vídeo mantêm o da tomada, então pulam esta checagem.

O Bardo mede o rosto da aparência uma vez por imagem, em segundo plano. Uma checagem que falha diz por quê e segura o teste, o rascunho e a versão final da tomada até você gravar ou cortar de novo, ou selecionar **Enviar mesmo assim**, que vale para o vídeo de controle aprovado dessa tomada. Uma checagem que ainda não pode ser lida (uma tomada analisada antes de a checagem existir, ou uma aparência ainda sendo medida) avisa isso e não segura nada; analisar a tomada de novo a roda.

<a id="take-prompt"></a>
## O prompt da tomada

**Escrever prompt** faz o Claude escrever o prompt da tomada a partir do modelo **Prompt da tomada** ([Modelos](templates.md#kinds)): a descrição da aparência (uma foto não tem), o cenário, as palavras da tomada (para a interpretação, nunca citadas) e o que o modelo faz com o vídeo de controle. A não ser que a tomada esteja em Profundidade sobre uma imagem, um quadro do meio do vídeo de controle vai junto, para o Claude poder descrever o seu ambiente (o prompt o descreve sem ninguém nele). Em Gravação, esse quadro é da sua gravação e mostra você; a tomada avisa isso antes de você escrever o prompt. Edite o prompt como quiser, até 2.500 caracteres; **Escrever de novo** o substitui, edições incluídas. Um prompt vale para o vídeo de controle e o modelo para os quais foi escrito: aprovar uma nova montagem ou escolher outro modelo pede um novo.

<a id="draft-final"></a>
## Rascunho e versão final

**Fazer rascunho** e **Fazer versão final** enviam o vídeo de controle, a aparência e o prompt ao provedor do modelo de tomada; a estimativa abaixo deles é por segundo da tomada (em tokens na BytePlus). A aparência vai como um JPEG que o Bardo faz a partir dela, o que deixa de fora o que uma foto registra sobre onde e com o que foi tirada, a não ser que vá pelo id do asset na BytePlus. O som do provedor fica desligado: rascunhos e versões finais tocam com a voz da tomada, do vídeo de controle.

- Um rascunho espera revisão: **Reproduzir rascunho**, depois **Aprovar rascunho**, ou **Refazer rascunho** para outro, que o substitui. **Voltar aos rascunhos** desfaz a aprovação.
- Quando a versão final é uma tomada nova (Seedance na Higgsfield, Kling direto do Kling), o rascunho é opcional: **Fazer versão final sem rascunho** faz a versão final direto, com a estimativa ao lado. No BytePlus ModelArk, cuja versão final completa o rascunho, só um rascunho aprovado libera **Fazer versão final**.
- A versão final espera revisão do mesmo jeito: **Reproduzir versão final**, depois **Usar**. **Fazer de novo** faz outra, que a substitui. Uma versão final é cobrada mesmo que você não a use.

Uma linha diz quanto o resultado saiu mais longo ou mais curto que a tomada. **Parar de esperar** faz o Bardo parar de esperar, não o provedor, que ainda pode terminar e cobrar: **Verificar de novo** retoma o mesmo pedido, como o Bardo faz depois de reiniciar, e nada novo pode ser feito para a tomada até lá. Quando o provedor recusa o conteúdo, ou o pedido falha do lado dele, a tomada diz isso (com o motivo do provedor, quando ele dá um) e nada é mudado nem enviado de novo sozinho: fazer de novo é um novo pedido pago, e só você o inicia.

<a id="byteplus"></a>
## No BytePlus ModelArk

O Seedance 2.5 direto do ModelArk precisa dos cinco campos do cartão **BytePlus ModelArk** ([Como obter cada chave de API](get-api-keys.md#byteplus)). O que muda lá:

- **A versão final completa o rascunho.** **Fazer versão final** envia ao ModelArk só a tarefa do rascunho aprovado, e o ModelArk faz a versão final com o prompt, as entradas e a semente do próprio rascunho. Aprovar outro rascunho, ou refazê-lo, faz dele o rascunho que a versão final completa.
- **Um prazo.** O ModelArk só completa um rascunho em até sete dias, então o rascunho mostra a data e a hora até quando fazer a versão final. Depois disso, **Fazer versão final** não aparece e a tomada pede um rascunho novo; um rascunho aprovado fora do prazo oferece **Refazer rascunho**.
- **O seu bucket.** O ModelArk só lê o vídeo de controle, e a aparência quando ela não vai pelo id do asset, por um link. O Bardo envia os dois ao seu bucket com nomes aleatórios em `bardo/takes/`, com links que duram no máximo sete dias, e os apaga assim que não são mais necessários: quando a versão final fica pronta, quando um rascunho novo substitui o antigo, quando um rascunho falha ou é recusado, quando a captura é removida e, no mais tardar, quando os sete dias do rascunho acabam. Uma exclusão que falha é tentada de novo depois; a regra de ciclo de vida do bucket é a garantia.
- **Uma foto sua precisa da sua semelhança verificada.** O Seedance recusa rostos reais, então uma tomada cuja persona tem uma foto como aparência só é enviada quando a aparência tem a sua semelhança verificada da biblioteca de assets do ModelArk ([A aparência](personas.md#look-asset)).
- **Custos em tokens.** O ModelArk cobra os tokens do vídeo: cerca de US$ 3,70 por um rascunho de 30 segundos e US$ 20 pela versão final dele. [Custos](costs.md) registra os tokens que o ModelArk informa.
- **Uma resposta perdida é achada de novo.** O ModelArk não tem como distinguir um pedido repetido de um novo, então o Bardo salva a hora logo antes de cada envio. Se o Bardo parar antes de a resposta do ModelArk chegar, a próxima tentativa procura esse pedido na lista de tarefas do ModelArk e espera por ele em vez de pagar de novo. Se achar vários, não envia nada e pede que você olhe a lista de tarefas no ModelArk.

<a id="kling"></a>
## Direto do Kling

Os modelos Kling 3.0 Motion Control direto do Kling usam a chave **Kling** ([Como obter cada chave de API](get-api-keys.md#kling)) e os pacotes de recursos do Kling. O que muda:

- **Preços.** O Kling cobra os segundos de vídeo que devolve: cerca de US$ 0,126 por segundo numa versão final em 720p, US$ 0,168 em 1080p, e US$ 0,07 num rascunho ou teste no Kling 2.6 em 720p. Uma tomada pode voltar um pouco mais curta do que foi enviada, e [Custos](costs.md) registra o que o Kling devolveu.
- **O seu armazenamento guarda o vídeo de controle.** O Kling recebe a aparência dentro do pedido, mas só lê o vídeo de controle por um link, e não guarda arquivos. O Bardo o envia para um armazenamento que você já configurou: o seu bucket da BytePlus quando a chave **BytePlus ModelArk** está salva (com um link assinado que dura no máximo sete dias, apagado assim que a tomada é feita), senão o upload da Higgsfield, que é grátis. Sem nenhuma das duas chaves, a tomada diz o que adicionar.
- **Uma resposta perdida é encontrada de novo.** Cada pedido leva um id próprio; se o Bardo parar antes de a resposta do Kling chegar, a próxima tentativa encontra esse pedido em vez de pagar de novo.

<a id="use"></a>
## Usar um clipe interpretado

**Usar** copia a versão final para a mídia do projeto como um clipe interpretado com o nome da tomada ("Clipe interpretado 1"), na aba **Mídia** do editor, onde **Pôr no corte** o coloca em sincronia com a narração da tomada ([Clipes interpretados](editor.md#performed)). Cada **Usar** adiciona um, e a tomada os conta. Os clipes interpretados ficam quando a captura é removida, e as versões em outros idiomas não os levam. Quando a aparência da persona é realista, um corte com um dos clipes interpretados dela liga o [aviso de conteúdo sintético](uploading.md#disclosure).

<a id="cuts"></a>
## Mover um corte

**Cortar antes** e **Cortar depois** movem o corte entre uma tomada e a seguinte para o fim de frase ou a pausa mais próxima, antes ou depois, que deixe as duas tomadas com 4 a 30 segundos. As duas tomadas mudam, e o que foi feito do trecho antigo delas aparece como desatualizado.

<a id="stale"></a>
## Desatualizada

Quando o roteiro muda depois da captura, a narração dela fica desatualizada, e a captura também: grave de novo com o roteiro atual e importe.

<a id="remove"></a>
## Remover

**Remover captura** tira a gravação e suas tomadas do projeto, com os rascunhos e as versões finais delas. A narração feita com ela e os clipes interpretados que você usou ficam. O Bardo pergunta uma vez antes; para ter as tomadas de volta, importe a gravação de novo.
