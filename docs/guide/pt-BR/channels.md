---
id: channels
title: Canais
group: publishing
place: channels
tour: channels
---

# Canais

Um canal é um nicho para um mercado: um assunto, um idioma, um país e um narrador. Todo o resto depende de um canal: as pesquisas, as ideias de vídeo, os projetos e as [contas de rede](network-accounts.md). Os canais ficam em [Canais](bardo:go/channels). [Mostre a tela para mim](bardo:tour/channels).

<a id="list"></a>
## Seus canais

A lista mostra cada canal com o nicho e o país. Escolha um para editá-lo ao lado da lista; **Novo canal** limpa o formulário para um canal novo. **Criar canal** (ou **Salvar alterações**) confere o formulário e diz, embaixo de cada campo, o que ainda falta.

<a id="niche"></a>
## Nicho e temas

- **Nicho**: do que o canal trata, em poucas palavras (por exemplo *história espacial*), até 100 caracteres. A [pesquisa](niche-research.md) dá nota a ele, e as [ideias de temas](themes-ranking.md) são escritas para ele.
- **Temas**: os assuntos a que o canal sempre volta, um por linha. Ideias e roteiros se apoiam neles, para um canal de história espacial ficar em missões, astronautas e descobertas.

<a id="look"></a>
## Notas de estética

Como os vídeos do canal devem parecer: o estilo visual, a paleta, o clima, o que evitar. Ideias e roteiros levam essas notas junto, e os prompts das cenas escritos a partir do roteiro também, para todo vídeo manter a cara do canal.

<a id="market"></a>
## Idioma e país

- **Idioma do conteúdo**: o idioma em que os roteiros, a narração e os textos dos posts do canal são escritos.
- **País-alvo**: o público que o canal quer alcançar. A [pesquisa](niche-research.md#market) lê tendências e concorrência desse país.

Cada conta de rede do canal ainda pode postar em outro idioma; veja os [padrões de metadados](network-accounts.md#metadata).

<a id="persona"></a>
## Persona padrão

A [persona](personas.md) que narra os vídeos do canal: a voz, o tom e o estilo de roteiro. Um projeto novo começa com ela, e o projeto pode escolher outro narrador. Uma persona pode narrar vários canais; **Sem persona padrão** deixa a escolha para cada projeto.

<a id="format"></a>
## Formato do vídeo

- **Short (9:16, menos de um minuto)**: um vídeo vertical de 45 a 60 segundos, que todas as redes aceitam. Canais novos começam assim.
- **Longo (16:9, 8 a 10 min)**: um vídeo deitado de 8 a 10 minutos, para o YouTube. As outras redes mantêm os presets delas (verticais, menos o 16:9 do Kick), e um corte mais longo do que uma delas aceita fica [bloqueado](render.md#gates) só para ela, então um vídeo longo é, na prática, um vídeo de YouTube.

O formato define o tamanho do roteiro que o Claude escreve, o formato em que as imagens e os clipes das cenas são feitos, o formato de um corte novo no editor e qual [preset de render](render.md#targets) o YouTube usa. Cada projeto segue o formato do canal, a não ser que escolha o outro ([Projetos](projects.md#format)). A mudança vale para o que for feito depois; roteiros, imagens e cortes já feitos mantêm o formato.

<a id="production"></a>
## Modo de produção

Como os vídeos do canal são feitos: quais cenas viram clipes de IA e com qual modelo, de só imagens com movimento (**Econômico**) a todas as cenas animadas em 1080p (**Tudo animado, premium**). Canais novos começam no **Gancho animado**, com os primeiros 10 segundos animados; canais criados antes dos modos ficam em **Tudo animado, modelo do canal**, então nada muda para eles até você escolher outro. Cada projeto segue o modo do canal, a não ser que escolha o seu, e mostra quanto o vídeo inteiro custa em cada modo antes de gerar qualquer coisa ([Projetos](projects.md#mode)).

<a id="defaults"></a>
## Modelo de vídeo, legendas, transições e movimento

- **Modelo de vídeo**: o modelo que anima as imagens das cenas do canal em clipes curtos, pelo Higgsfield, pelo Kling ou pelo Google, no [modo de produção](#production) **Tudo animado, modelo do canal**; os outros modos escolhem o seu modelo. Manter o padrão segue o primeiro provedor, na ordem Higgsfield, Kling, Google, com chave salva, e o modelo padrão desse provedor quando ele muda: só com uma chave do Kling ou do Gemini, os clipes vão para o Kling ou para o Veo, nunca para um provedor sem chave. Qualquer cena pode escolher outro modelo na etapa [Clipes](clips.md#model).
- **Modelo de tomada**: o modelo que faz a persona interpretar cada tomada de uma captura, pela Higgsfield, pelo BytePlus ModelArk ou pelo Kling. Manter o padrão segue o primeiro deles, nessa ordem, com chave salva: o Kling 3.0 Motion Control na Higgsfield, o Seedance 2.5 Video Edit no BytePlus ModelArk ou o Kling 3.0 Motion Control 720p no Kling. Qualquer tomada pode escolher outro ([Clipe interpretado](capture.md#performed)).
- **Estilo de legenda**: o visual das legendas nos projetos novos do canal. Cada projeto pode escolher outro estilo no [editor](editor.md#captions).
- **Palavra falada**: a cor em que acende a palavra sendo dita, nos projetos novos do canal. **Amarelo** é o padrão de um canal novo; **Desligado** é onde começam os canais criados antes disso existir, para que seus cortes e renders fiquem como estavam. Cada projeto pode escolher outra cor no [editor](editor.md#captions).
- **Transição padrão** e a **Duração** dela: o que o editor põe num corte quando você aperta **T** ou escolhe uma transição para um clipe; uma fusão de 0,5 s, a menos que você mude. Cada corte pode usar outra ([Transições](editor.md#transitions)).
- **Movimento nas imagens** e a **Intensidade** dele: como a câmera passeia por cada imagem parada num corte novo, o efeito Ken Burns. **Variado** (o padrão de um canal novo, com intensidade **Média**) dá a cada cena um zoom ou pan diferente do anterior; **Nenhum** deixa as imagens paradas, e é onde começam os canais criados antes de existir movimento, para os cortes e renders deles ficarem como estavam; ou escolha um movimento para todas as cenas. Cortes já feitos guardam os movimentos que têm, e cada imagem pode usar outro ([Movimento nas imagens](editor.md#motion)).
- **Estilo da miniatura**: como fica o texto das miniaturas novas do canal no YouTube, Forte a menos que você mude. Cada vídeo pode escolher outro ([Miniatura](uploading.md#thumbnail)).

<a id="accounts"></a>
## Depois: as contas de rede

Um canal posta pelas suas contas, uma por rede. Adicione-as em [Contas](bardo:go/accounts): cada uma tem o @ com que posta, os metadados de que todo post parte e o preset de render dos arquivos. Veja [Contas de rede](network-accounts.md).
