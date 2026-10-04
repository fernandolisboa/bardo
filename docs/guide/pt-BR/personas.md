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

A lista traz cada persona, com a voz dela. O Bardo vem com quatro, duas em inglês e duas em português; **Nova persona** cria a sua, e escolher uma a abre para edição. Salvar uma persona que alguns canais usam como padrão muda o narrador deles também, então o Bardo diz quais são esses canais e pergunta antes. Para mudar o narrador de um canal só, **Duplique** a persona e edite a cópia.

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

**Criar persona** ou **Salvar alterações** guarda a persona. **Exportar para arquivo…** salva um pacote com a persona e uma referência à voz, nunca áudio ou chaves, para usar em outro computador; **Importar de arquivo…**, no topo, traz uma para cá. Quem importa precisa da mesma voz na própria conta da ElevenLabs: até o Bardo encontrá-la lá, a persona fica marcada como **Voz não verificada** ou **Voz indisponível** e não pode narrar.

<a id="realistic"></a>
## Vozes realistas

Marque **Voz sintética realista ou clone de uma pessoa real** quando a voz soa como uma pessoa real: um clone, ou uma voz profissional feita com gravações de alguém. Escolher uma voz clonada ou profissional já marca a opção. As redes pedem que esses vídeos sejam marcados como conteúdo alterado ou sintético, e o Bardo lembra você na exportação.
