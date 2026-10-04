---
id: first-video
title: Seu primeiro vídeo
group: getting-started
---

# Seu primeiro vídeo

Esta página acompanha um vídeo desde o Bardo vazio até o post publicado. Cada passo é uma tela, e os links abrem a tela. Salve antes as suas [chaves de API](api-keys.md), porque quase todo passo chama um provedor.

<a id="channel"></a>
## 1. Crie um canal

O canal é a sua marca: nome, nicho, os temas que ele cobre, o visual, o idioma em que fala e o país que quer alcançar. Em [Canais](bardo:go/channels), escolha **Novo canal** e preencha. Escolha também um narrador padrão e adicione a conta do canal em cada rede onde vai postar; essas contas guardam o padrão dos posts de cada rede (tags, rodapé, visibilidade) e os ajustes de render.

<a id="persona"></a>
## 2. Escolha um narrador

Uma persona é um narrador: uma voz da sua conta na ElevenLabs, um tom, um estilo de escrita e os ajustes da voz. O Bardo já vem com quatro, duas em inglês e duas em português. Em [Personas](bardo:go/personas) você ouve uma voz antes de usar, ajusta, ou cria a sua com qualquer voz da sua conta na ElevenLabs, clones incluídos. As personas são suas, não de um canal, então uma pode narrar para vários canais.

<a id="research"></a>
## 3. Pesquise um nicho

Em [Pesquisa](bardo:go/research), escolha o canal e digite alguns nichos ou palavras-chave de partida. Uma tarefa busca os envios recentes no YouTube para cada um e dá as notas: **concorrência** (o quanto é difícil se destacar), **tendência** (a rapidez com que vídeos novos ganham views) e **oportunidade**, que pesa as duas. Os resultados ficam guardados por sete dias, então olhar de novo não gasta cota.

<a id="theme"></a>
## 4. Escolha uma ideia de vídeo

Em [Temas](bardo:go/themes), peça ideias para um nicho. O Claude sugere títulos e ângulos, e o motor de decisão ranqueia as ideias pelo encaixe com o canal, pela tendência e pela concorrência, e também pelo desempenho dos seus vídeos quando já houver algum. Edite uma ideia, descarte ou aprove: **aprovar abre um projeto de vídeo**.

<a id="project"></a>
## 5. Produza, etapa por etapa

Um [projeto](bardo:go/projects) passa por etapas, mostradas no alto. Cada uma pode ser revisada e alterada antes da próxima:

- **[Roteiro](bardo:go/projects/script)**: o Claude escreve a partir da ideia, da persona e do canal. Edite à vontade; uma versão nova espera ao lado da atual até você aceitar.
- **[Narração](bardo:go/projects/narration)**: a persona lê o roteiro, com o tempo de cada palavra. Você também pode importar a sua própria gravação.
- **[Cenas](bardo:go/projects/scenes)**: o Claude divide a narração em cenas, cada uma com um prompt de imagem que você pode editar, e cada cena ganha a sua imagem.
- **[Clipes](bardo:go/projects/clips)**: vídeos curtos opcionais feitos a partir da imagem de uma cena, com o custo mostrado antes de gerar.

Quando um roteiro ou uma narração muda, o que foi feito a partir deles aparece como desatualizado, então você sempre sabe o que gerar de novo.

<a id="editor"></a>
## 6. Edite

A etapa **Edição** abre o editor com um primeiro corte já montado a partir das cenas e da narração. Divida, apare, mova e apague clipes; os cortes se encaixam nas palavras da narração. Mixe a narração com música e efeitos sonoros, edite as legendas e escolha o formato do quadro (16:9 ou 9:16). As sugestões de corte marcam bons pontos para cortar; aceitar uma é só mais uma edição, e dá para desfazer.

O Bardo não faz música. No projeto, ele escreve um prompt de música para a sua ferramenta; importe a faixa que você fizer na aba Mídia do editor.

<a id="render"></a>
## 7. Revise e renderize

**Revisar e renderizar** mostra o corte, o volume da mixagem, as legendas e, para cada conta do canal nas redes, o formato e os problemas encontrados (longo demais para a rede, legendas desligadas, mixagem baixa demais). Escolha as redes e confirme: o render roda como tarefa, e você continua trabalhando enquanto isso.

<a id="publish"></a>
## 8. Publique

Na etapa [Publicação](bardo:go/projects/publish), o Claude escreve o título, a descrição e as tags de cada rede, e você edita dentro dos limites de cada uma. Depois, uma de duas:

- **Exportar** uma pasta por rede com o arquivo e um texto para copiar, e postar à mão; cole de volta o link do post para o Bardo acompanhar os números.
- **Enviar** para o YouTube, o Instagram Reels ou o TikTok (como rascunho que você termina no app do TikTok), depois de conectar a conta em [Configurações › Redes](bardo:go/settings/networks). Todo envio passa antes por uma revisão.

<a id="after"></a>
## 9. Veja como foi

[Desempenho](bardo:go/performance) mostra os números de cada vídeo e como a primeira semana dele se compara com o normal do canal. O ranking de temas também lê esses números, então as próximas ideias são pesadas pelo que deu certo.
