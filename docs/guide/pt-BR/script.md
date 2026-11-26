---
id: script
title: Roteiro
group: production
place: videos/script
tour: script
---

# Roteiro

O roteiro é o que o narrador vai dizer. O Claude escreve, você edita, e todas as etapas seguintes partem dele. Ele fica na etapa [Roteiro](bardo:go/videos/script) de um projeto, com a música do vídeo logo abaixo: o prompt dela e as faixas feitas a partir dele. [Mostre a etapa para mim](bardo:tour/script).

<a id="write"></a>
## Gere e depois edite

**Gerar roteiro** pede ao Claude um roteiro montado a partir do **modelo de roteiro** ([Modelos](templates.md)), do canal (nome, nicho, temas, visual e idioma), do tema aprovado, do tom e do estilo de roteiro do narrador e do [formato](projects.md#format) do vídeo: de 45 a 60 segundos de narração para um Short, de 8 a 10 minutos para um vídeo longo, a menos que o projeto escolha a própria [duração](projects.md#length). O modelo pede um único fio condutor do começo ao fim, uma história na ordem em que aconteceu ou uma explicação construída passo a passo, escrita de um jeito que dê para mostrar, porque as [cenas](scenes.md#plan) são planejadas a partir dela. A geração roda como tarefa, então você pode continuar trabalhando; é preciso uma chave do Claude ([Suas chaves de API](api-keys.md#providers)).

Um [anúncio](brief.md) é escrito a partir do **modelo de roteiro de anúncio**, com o que o briefing dele guarda: o produto, com quem o anúncio fala, o ângulo e o formato dele, as afirmações numeradas do briefing, a oferta, a chamada para ação e o que precisa e o que nunca pode ser dito. O modelo pede o gancho nos dois primeiros segundos, o produto citado no máximo até a metade, frases curtas, de cerca de quatro a nove palavras, e a chamada para ação por último, com as suas palavras, numa frase só dela. O roteiro de um anúncio mira 2 segundos a menos que a duração do projeto, para o vídeo terminar no tempo: cerca de 26 a 32 palavras em 15 segundos, cerca de 58 a 70 em 30. Ele só pode dizer o que as afirmações do briefing dizem, e a [checagem de afirmações](#claims) lê o roteiro assim que ele é escrito. Num produto do tipo *ideia*, o Claude é avisado de que se trata de um conceito: o roteiro fala do que ele faria e nunca diz que dá para comprar ou usar hoje.

Depois que o roteiro chega aqui, ele é seu: digite direto nele e use **Salvar** ([Salvar as suas edições](#save)). Cabem até 60.000 caracteres, e a contagem de palavras ao lado do título dá uma ideia da duração do vídeo.

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

Antes de gerar, a estimativa sob os botões diz quanto a chamada vai custar pelas tarifas do provedor. Num anúncio, ela inclui a checagem de afirmações que vem depois do roteiro. Se a chamada passaria de um dos seus orçamentos, o Bardo pergunta antes de começar. Quanto o projeto já gastou fica sob o nome dele ([Projetos e etapas](projects.md#cost)).

<a id="claims"></a>
## A checagem de afirmações

Só um anúncio tem. A cada roteiro de anúncio que o Claude escreve, a mesma tarefa faz uma segunda chamada, pequena, que lê o roteiro e lista o que ele diz sobre o produto: o que é, o que faz, quanto custa, qualquer número, comparação, resultado ou promessa. Cada ponto fica ligado a uma afirmação do [briefing](brief.md#claims), ou a nenhuma. O cartão **Checagem de afirmações**, sob o roteiro, mostra o resultado:

- **Tudo o que o roteiro diz tem base**, ou quantos pontos estão sem base. Cada um deles aparece com a frase que o diz e, embaixo, o que ela diz. Um roteiro que não diz nada sobre o produto mostra **Não diz nada sobre o produto**.
- **Afirmações que este roteiro usa**, e, separadas, as **Afirmações sensíveis que este roteiro usa**: as marcadas como [sensíveis](products.md#claims) no produto, que pedem cuidado.

Um ponto sem base não trava nada: você decide se a frase fica, muda ou sai. A etapa Narração avisa, acima da estimativa, quando o roteiro que ela vai ler tem pontos sem base ou não foi conferido como está agora ([Narração](narration.md#generate)).

A checagem vale para o roteiro e o briefing que ela leu. Salve uma edição, mude o briefing ou mude o que o próprio produto diz que precisa ser dito, e o cartão mostra **Desatualizada**; **Conferir de novo** confere o roteiro salvo e substitui a checagem anterior. É uma chamada paga pequena, com a estimativa sob o botão. A checagem lê o roteiro como está salvo: enquanto o editor tiver edições que você não salvou, o botão espera e uma linha pede para salvar antes. Um roteiro que ainda não tem checagem (um anúncio escrito antes de a checagem existir, ou uma checagem que falhou) mostra **Este roteiro ainda não foi conferido**, e o botão é **Conferir afirmações**.

Um roteiro novo à espera de revisão mostra a própria checagem dentro do cartão dele, acima de **Aceitar o novo roteiro**, e aceitar o roteiro mantém essa checagem com ele. A checagem acompanha o texto que ela leu: ao aceitar um roteiro novo, a checagem do roteiro atual vai embora com o roteiro a que pertencia.

Enquanto uma checagem roda, o cartão avisa. Se a tarefa escreveu o roteiro e depois falhou na checagem, a etapa diz **O roteiro foi escrito, mas a checagem de afirmações parou**; rodar a tarefa de novo em [Tarefas](jobs.md) paga só a checagem, e pergunta antes se isso passaria de um orçamento.

A checagem não é um modelo editável: as instruções dela são fixas, então nenhuma edição faz com que ela faça vista grossa. Um ponto que a resposta liga a uma afirmação que o briefing não tem conta como sem base.

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

<a id="versions"></a>
## Versões em outros idiomas

No fim da etapa Roteiro, **Versões em outros idiomas** faz o mesmo vídeo de novo para outro canal seu com outro idioma ou país, como um canal em inglês para os Estados Unidos e um em português para o Brasil. Antes, o vídeo precisa da narração e das cenas planejadas nessa narração. Cada canal possível aparece com o idioma, o país e o narrador (a persona padrão do canal) e quanto custa a adaptação; **Fazer versão** inicia o job.

O Claude adapta em vez de traduzir, a partir do **modelo de localização** ([Modelos](templates.md#kinds)): expressões, unidades, moedas, datas e referências do novo país, com os fatos mantidos e cada frase com mais ou menos o mesmo tempo de fala. Ele responde com um título e uma frase adaptada para cada frase da narração, para que cada cena siga a sua frase; qualquer outra resposta faz o job falhar e nada é criado. Só então a versão começa como um projeto desse canal, com:

- o roteiro e o título adaptados, para você revisar e editar como qualquer roteiro;
- cópias do plano de cenas (imagens e clipes atuais, prompts e modelos de vídeo escolhidos) e das mídias e músicas geradas do projeto, cada uma com a sua origem;
- o que o corte fazia com elas, para quando as cenas forem posicionadas ([Posicionar as cenas numa versão](scenes.md#place)).

Os arquivos são copiados para a pasta da própria versão, então remover um projeto nunca mexe no outro. Narração, legendas, metadados, miniatura, capítulos, renderizações e publicações não são copiados: a versão faz os seus, para o seu público, como o YouTube espera de conteúdo reaproveitado. A versão mantém o formato do original, Short ou longo. Só a adaptação é cobrada, no novo projeto ([Custos](costs.md)).

Um canal tem uma versão de cada vídeo; depois de feita, ela aparece em **Versões feitas** com **Abrir**. Na versão, a etapa Roteiro diz de qual vídeo ela é versão, e **Gerar de novo** adapta as frases do original outra vez; a nova adaptação aguarda revisão como qualquer roteiro novo. Remover o original mantém as versões.
