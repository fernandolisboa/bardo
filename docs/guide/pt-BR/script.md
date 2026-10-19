---
id: script
title: Roteiro
group: production
place: projects/script
tour: script
---

# Roteiro

O roteiro é o que o narrador vai dizer. O Claude escreve, você edita, e todas as etapas seguintes partem dele. Ele fica na etapa [Roteiro](bardo:go/projects/script) de um projeto, com a música do vídeo logo abaixo: o prompt dela e as faixas feitas a partir dele. [Mostre a etapa para mim](bardo:tour/script).

<a id="write"></a>
## Gere e depois edite

**Gerar roteiro** pede ao Claude um roteiro montado a partir do **modelo de roteiro** ([Modelos](templates.md)), do canal (nome, nicho, temas, visual e idioma), do tema aprovado e do tom e do estilo de roteiro do narrador. A geração roda como tarefa, então você pode continuar trabalhando; é preciso uma chave do Claude ([Suas chaves de API](api-keys.md#providers)).

Depois que o roteiro chega aqui, ele é seu: digite direto nele. Cabem até 60.000 caracteres, e a contagem de palavras ao lado do título dá uma ideia da duração do vídeo.

<a id="review"></a>
## Uma nova versão para revisar

**Gerar de novo** pede um roteiro inteiramente novo sem mexer no seu. O novo espera acima dele como **Novo roteiro para revisar**:

- **Aceitar o novo roteiro** substitui o atual, inclusive as suas edições.
- **Manter o atual** descarta o novo.

Até você decidir, a etapa Roteiro mostra **Nova versão para revisar**.

<a id="save"></a>
## Salvar as suas edições

**Salvar** guarda o que você digitou, e o roteiro fica marcado como **Editado**. **Descartar alterações** volta ao roteiro salvo por último. O que foi feito a partir de um roteiro anterior, como a narração, passa a aparecer como **Desatualizada** ([Narração](narration.md#stale)).

<a id="cost"></a>
## Quanto custa

Antes de gerar, a estimativa sob os botões diz quanto a chamada vai custar pelas tarifas do provedor. Se a chamada passaria de um dos seus orçamentos, o Bardo pergunta antes de começar. Quanto o projeto já gastou fica sob o nome dele ([Projetos e etapas](projects.md#cost)).

<a id="details"></a>
## De onde ele veio

**Detalhes** lista o provedor, o modelo de IA, o modelo de roteiro e a versão dele, os tokens usados e quando o roteiro foi gerado. **Mostrar prompt** mostra as instruções e o prompt exatamente como foram enviados. Um novo roteiro à espera de revisão tem os próprios detalhes.

<a id="music"></a>
## O prompt de música

Abaixo do roteiro, **Gerar prompt de música** pede ao Claude um prompt para a música do vídeo, a partir do **modelo de prompt de música**, do canal, do tema e da duração do vídeo. Edite o prompt e **Salve**; **Gerar de novo** o substitui, inclusive as suas edições. Faça a música a partir dele logo abaixo ([Música a partir do prompt](#generated-music)), ou **Copie** o prompt para uma ferramenta que você tenha direito de usar e importe a faixa na aba Mídia do editor.

<a id="generated-music"></a>
## Música a partir do prompt

Abaixo do prompt, **Gerar música** pede ao ElevenLabs Music uma faixa instrumental composta a partir do prompt salvo, com a duração da narração (de 3 segundos a 10 minutos). Faça a narração primeiro. Roda como tarefa e precisa de uma chave da ElevenLabs com a permissão **Music** num plano pago da ElevenLabs ([Como obter cada chave de API](get-api-keys.md#elevenlabs)). A estimativa ao lado do botão é calculada por minuto de música, e uma faixa que passaria do orçamento da ElevenLabs pergunta antes ([Custos e orçamentos](costs.md)).

A faixa nova espera no cartão, marcada como **Faixa nova**, com a duração e o modelo. **Ouça** e então:

- **Usar esta faixa** coloca a faixa na trilha de música (A2) desde 0:00, abaixada sob a narração como o seu corte já faz, no lugar da faixa gerada usada antes. A música que você mesmo colocou fica. Todo corte feito depois (o primeiro corte, ou um corte que recomeça depois de cenas ou narração novas) já começa com a faixa em uso.
- **Descartar** apaga a faixa.

**Gerar outra** faz uma faixa nova, que toma o lugar da que espera revisão; a faixa em uso fica até você usar outra. Uma faixa usada é um arquivo de mídia do projeto, então também aparece na aba Mídia do editor ([O editor](editor.md)), onde dá para colocá-la de novo à mão. Uma faixa esperando revisão não aparece.

Cada faixa guarda o prompt, a duração pedida, o modelo e a tarefa que a fez. A ElevenLabs assina a faixa com credenciais de conteúdo C2PA, que dizem que ela foi feita por IA. Uma tarefa que falhou mostra o motivo dado pela ElevenLabs: plano sem acesso ao Music, chave sem a permissão Music, créditos esgotados ou um prompt recusado (por exemplo, um que cita um artista; a ElevenLabs sugere outra redação).
