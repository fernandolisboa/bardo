---
id: personas
title: Personas
group: production
place: personas
tour: personas
---

# Personas

Uma persona é um narrador: uma voz da sua conta na ElevenLabs, um tom, um estilo de roteiro e os ajustes com que a ElevenLabs lê. As personas são suas, não de um canal: uma pode narrar para vários canais, e cada canal tem uma padrão ([Projetos e etapas](projects.md#narrator)). Elas ficam em [Personas](bardo:go/personas). [Mostre a tela para mim](bardo:tour/personas).

<a id="library"></a>
## Seus narradores

A lista traz cada persona, com a voz dela. O Bardo vem com quatro, duas em inglês e duas em português; **Nova persona** cria a sua, e escolher uma a abre para edição. Salvar uma persona que alguns canais usam como padrão muda o narrador deles também, então o Bardo diz quais são esses canais e pergunta antes. Para mudar o narrador de um canal só, **Duplique** a persona e edite a cópia; a cópia fica com a aparência.

<a id="voice"></a>
## A voz

**Escolher entre minhas vozes da ElevenLabs** lista as vozes da sua conta na ElevenLabs, clones incluídos; cada uma tem uma prévia gratuita da ElevenLabs. A persona guarda só uma referência à voz, nunca áudio ou chaves. Sob a voz escolhida, o Bardo diz se ela ainda está na sua conta; narrar com uma voz que sumiu falharia. É preciso uma chave da ElevenLabs ([Suas chaves de API](api-keys.md#providers)).

<a id="style"></a>
## Tom e estilo de roteiro

O **Tom** diz como o narrador soa ("sóbrio, comedido, sem exagero"), e o **Estilo de roteiro**, como os roteiros são escritos para ele: estrutura, tamanho das frases, ganchos. O Claude lê os dois sempre que escreve o roteiro de um vídeo que esta persona narra.

<a id="presets"></a>
## Presets de geração

Os presets definem como a ElevenLabs lê com esta voz:

- **Estabilidade**: mais alto é mais estável e uniforme; mais baixo é mais expressivo.
- **Similaridade**: o quanto a narração se mantém fiel à voz original.
- **Exagero de estilo**: amplifica o estilo próprio da voz; 0 desliga.
- **Velocidade**: 100% é o ritmo normal da voz, de 70% a 120%.

<a id="sample"></a>
## Ouvir antes

**Ouvir com estes ajustes** lê uma frase curta (que você pode mudar, até 250 caracteres) com a voz escolhida e os presets como estão agora, salvos ou não. Cada combinação nova é uma chamada paga curta à ElevenLabs, registrada em Custos; uma combinação que você já ouviu toca de novo de graça, a partir deste computador.

<a id="share"></a>
## Salvar e compartilhar

**Criar persona** ou **Salvar alterações** guarda a persona. **Exportar para arquivo…** salva um pacote com a persona, uma referência à voz e a aparência quando ela é desenhada, nunca áudio, chaves ou uma foto sua, para usar em outro computador; **Importar de arquivo…**, no topo, traz uma para cá. Quem importa precisa da mesma voz na própria conta da ElevenLabs: até o Bardo encontrá-la lá, a persona fica marcada como **Voz não verificada** ou **Voz indisponível** e não pode narrar.

<a id="realistic"></a>
## Vozes realistas

Marque **Voz sintética realista ou clone de uma pessoa real** quando a voz soa como uma pessoa real: um clone, ou uma voz profissional feita com gravações de alguém. Escolher uma voz clonada ou profissional já marca a opção. As redes pedem que esses vídeos sejam marcados como conteúdo alterado ou sintético, e o Bardo lembra você na exportação.

<a id="look"></a>
## A aparência

A aparência é como esta persona aparece nos clipes de atuação. Uma persona sem aparência não pode atuar. Há dois tipos:

- **Desenhada**: descreva o personagem em **Descreva o personagem** e escolha **Desenhar**. O Claude escreve um prompt de imagem a partir da sua descrição com o [modelo de prompt de aparência](templates.md), só para personagens originais e adultos: ele recusa uma pessoa real e identificável, uma criança ou um personagem protegido por direitos autorais e diz por quê, mesmo que o modelo tenha sido editado. Depois o Nano Banana desenha em formato vertical 2:3. As duas chamadas são estimadas antes de começar e registradas em [Custos](costs.md). O desenho espera revisão em **Novo desenho para revisar**: **Ficar com esta aparência** faz dele a aparência, **Descartar** joga fora, e **Desenhar de novo** troca por um novo. Você precisa das chaves do Claude e do Gemini ([Suas chaves de API](api-keys.md#providers)).
- **Foto sua**: **Importar uma foto minha…** aceita um PNG ou JPEG em que só você aparece, da cintura para cima, de frente para a câmera, com fundo liso, de 300 a 6.000 px de lado e até 10 MB. Marque antes **Só eu apareço nesta foto**. Ela vira a aparência na hora e nunca vai num pacote.

**Aparência realista** fica sempre marcada para uma foto sua, e marcada para um desenho fotorrealista: vídeos em que uma aparência realista atua precisam do rótulo de conteúdo alterado ou sintético das redes. **Remover aparência** apaga depois de perguntar; uma aparência desenhada só volta com um novo desenho. As mudanças nesta seção são salvas na hora, sem **Salvar alterações**. Uma persona importada de um pacote com aparência desenhada a traz como um desenho para revisar.

<a id="look-asset"></a>
## Biblioteca de assets da BytePlus

O Seedance 2.5 no BytePlus ModelArk recusa rostos reais no que recebe, e pode usar uma aparência da sua biblioteca de assets do ModelArk pelo id dela no lugar da imagem ([Como obter cada chave de API](get-api-keys.md#byteplus) explica como adicionar uma). Em **Biblioteca de assets da BytePlus**, escolha o **Tipo**, cole o **Id do asset** (com ou sem `asset://`) e escolha **Salvar asset**:

- Uma **foto sua** só aceita a sua **Semelhança verificada**, a que o ModelArk verificou como sendo você. Enquanto ela não tiver uma, as tomadas dela não podem ser enviadas à BytePlus.
- Uma **aparência desenhada** pode usar um **Personagem pronto** da biblioteca do ModelArk ou um **Retrato virtual** que você fez lá, ou ir como imagem, sem asset nenhum.

Um id vale só para esta aparência: deixa de valer quando você escolhe um desenho ou uma foto nova. **Remover asset** volta a enviar a imagem. O id fica neste computador: nunca vai num pacote de persona nem no arquivo de configurações, e duplicar a persona não o copia.
