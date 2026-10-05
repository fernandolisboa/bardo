---
id: glossary
title: Glossário
group: reference
---

# Glossário

As palavras que o Bardo usa, em termos simples.

<a id="strategy"></a>
## Estratégia

**Canal**: a sua marca: o nicho, os temas que cobre, o visual, o idioma, o país de destino e o narrador padrão. Tem uma conta em cada rede onde posta.

**Nicho**: um mercado de conteúdo, como "história do espaço".

**Mercado**: o país de destino do canal mais o idioma em que ele fala. A pesquisa é feita por nicho e mercado.

**Pesquisa de nicho**: uma tarefa que busca os envios recentes no YouTube para os nichos de partida e dá nota a cada um. Os resultados ficam guardados por sete dias.

**Concorrência**: de 0 a 100, o quanto é difícil se destacar num nicho. Muitos envios, canais grandes e poucas views por vídeo fazem a nota subir.

**Tendência**: de 0 a 100, a rapidez com que os vídeos recentes de um nicho ganham views.

**Oportunidade**: a média entre a tendência e o espaço que a concorrência deixa. A pesquisa ordena por ela.

**Tema**: uma ideia de vídeo dentro de um nicho: um título e um ângulo. Não confunda com o tema da interface, que é a aparência do Bardo.

**Ranking de temas**: por que o motor de decisão coloca uma ideia onde ela está: encaixe com o canal, tendência e concorrência, cada um com nota e confiança, e também o desempenho passado quando o canal já tem vídeos publicados.

**Desempenho passado**: como foram os vídeos do próprio canal, do jeito que o ranking lê: a primeira semana de cada vídeo comparada com o normal do canal.

**Primeira semana**: as views de um vídeo sete dias depois de ele ir ao ar.

<a id="production"></a>
## Produção

**Persona**: um narrador que é seu: uma voz, um tom, um estilo de escrita e os ajustes da voz. O canal tem uma persona padrão, e cada vídeo pode usar outra.

**Amostra de voz**: um trecho curto para ouvir uma voz antes de narrar com ela. A prévia padrão do provedor é grátis; a leitura de uma frase sua com os ajustes da persona é uma chamada paga.

**Pacote de persona**: uma persona salva num arquivo `.bardo-persona` para levar a outro computador. Nunca leva áudio nem chaves.

**Projeto de vídeo**: um vídeo em produção, do roteiro aos posts. Começa quando você aprova uma ideia.

**Modelo**: as instruções que o Bardo manda para a IA escrever um tipo de texto (o roteiro, os prompts de imagem, os textos dos posts). Salvar uma mudança cria uma versão nova, e as antigas continuam legíveis.

**Roteiro**: o que o narrador fala. O Claude escreve, e você edita como quiser.

**Narração**: o roteiro lido em voz alta pela persona do projeto, com o tempo de cada palavra. Se o roteiro muda, ela fica desatualizada.

**Tempo das palavras**: quando cada palavra da narração é falada. É o que conduz as legendas, a palavra destacada e onde os cortes se encaixam.

**Plano de cenas**: a narração dividida em cenas, cada uma com um prompt de imagem.

**Cena**: um trecho da narração com uma imagem e, se quiser, um clipe.

**Clipe**: um vídeo curto que um modelo de vídeo faz a partir da imagem de uma cena. Opcional.

**Prompt de movimento**: como a imagem da cena se move no clipe: o assunto, a ação, a câmera.

**Modelo de vídeo**: o modelo que faz os clipes, como o Kling pela Higgsfield ou o Veo pelo Gemini. Definido por canal e trocável por cena.

**Geração**: o registro de como algo foi feito: provedor, modelo, prompt e versão do modelo de instruções. Editar o resultado não muda esse registro.

**Prompt de música**: um prompt que o Claude escreve para a sua ferramenta de música. O Bardo não faz música; você importa a faixa.

<a id="editing"></a>
## Edição

**Linha do tempo**: a trilha de vídeo e as trilhas de áudio (narração, música, efeitos sonoros), com cortes, volumes, fades e legendas.

**Corte**: as suas edições na linha do tempo: dividir, aparar, mover, reordenar e apagar. Salvas enquanto você trabalha, e dá para desfazer enquanto o editor está aberto.

**Sugestão de corte**: um ponto onde o motor de decisão acha que a imagem poderia cortar, com uma nota e os motivos. Nada muda até você aceitar.

**Mixagem**: o volume de cada trilha de áudio, com mudo e solo.

**Ducking**: abaixar a música enquanto o narrador fala, na medida que você escolher.

**Legenda**: uma linha de texto na tela tirada da narração, no tempo das palavras, com um estilo por canal ou por projeto.

**Enquadramento**: o formato do quadro (16:9 ou 9:16) e como cada clipe preenche esse quadro.

**Proxy**: uma cópia leve de um clipe que a prévia reproduz, para a edição ficar fluida. O render sempre usa os originais.

**Preset de render**: o formato de saída de uma rede: proporção, resolução, codec, bitrate, duração máxima e volume.

**Render**: gerar o arquivo final do vídeo para cada rede.

**Revisão do render**: o que você confere antes de renderizar: o corte, a mixagem, as legendas e cada rede de destino.

**Verificação de qualidade**: um problema que a revisão do render aponta, como um vídeo longo demais para uma rede (que bloqueia aquela rede) ou legendas desligadas (um aviso).

<a id="publishing"></a>
## Publicação

**Rede**: uma plataforma social: YouTube, TikTok, Instagram Reels, X ou Kick.

**Conta na rede**: o perfil de um canal numa rede, com o padrão dos posts e os ajustes de render.

**Credenciais do app**: o ID e a chave secreta do app que você registra numa rede para o Bardo postar por você. O Bardo não traz nenhum app próprio.

**Conexão com a rede**: uma conta na rede com login feito pela própria rede, para o Bardo enviar vídeos e ler os números. Os logins ficam no Gerenciador de Credenciais do Windows.

**Metadados do vídeo**: o texto do post de um vídeo numa rede: título, descrição e tags, dentro dos limites da rede.

**Exportação**: uma pasta por rede com o arquivo renderizado e um texto para copiar, para postar à mão.

**Revisão do envio**: o que você confirma antes de um envio: o arquivo, o texto do post, a conta, a visibilidade e o aviso de IA.

**Publicação**: um vídeo enviado ou agendado para uma conta na rede, enviado pelo Bardo ou postado por você e ligado pelo endereço do post.

**Aviso de conteúdo sintético**: o selo que as redes pedem em vídeos com uma voz de IA realista. O Bardo liga o aviso quando a narração usou uma persona marcada como voz realista.

**Horário de publicação**: quando o próprio Bardo publica um post agendado no Instagram. O Bardo precisa estar aberto nessa hora, ou o agente em segundo plano rodando.

**Agente em segundo plano**: uma parte opcional do Bardo que o Windows inicia quando você se conecta, para enviar os posts no horário de publicação com o Bardo fechado. Liga e desliga em Configurações › Publicação.

**Post perdido**: um post agendado cujo horário passou sem o Bardo nem o agente em segundo plano rodando. O Bardo lista o post quando abre, e você escolhe o que fazer.

<a id="numbers"></a>
## Números, tarefas e dinheiro

**Registro de métricas**: os números de um post num momento. O Bardo guarda o histórico deles.

**Métricas do dono**: o que só o dono do canal vê no YouTube, como tempo assistido e receita, lido pela conta conectada. Chegam com dois ou três dias de atraso.

**Insights do post**: os números do dono para um post do Instagram ou do TikTok, lidos pela conta conectada.

**Sincronização de métricas**: a tarefa que lê os números dos seus posts, quando você pede e quando o Bardo abre.

**Tarefa**: trabalho longo (uma narração, imagens, um render, um envio) que roda em segundo plano, com progresso, cancelamento e retomada.

**Provedor**: um serviço pago de IA ou de dados que o Bardo chama com a sua chave.

**Chave do provedor**: a sua chave de API de um provedor, guardada no Gerenciador de Credenciais do Windows.

**Orçamento**: um limite mensal de gasto por provedor. Quando chega nele, as tarefas novas desse provedor perguntam antes de começar.

**Motor de decisão**: a IA que ranqueia e dá notas (ideias, cortes) com respostas tipadas e uma confiança. Nunca escreve conteúdo.

<a id="interface"></a>
## O app

**Layout**: onde cada tela posiciona as suas partes: Workspace ou Studio. Nunca muda o que a tela faz.

**Tema da interface**: as cores, os cantos e a fonte do Bardo. São dez, de claros e escuros a alto contraste e visuais de terminal.

**Tour guiado**: um passeio pelo Bardo com os seus próprios dados: a janela escurece e uma parte de cada vez fica acesa, com um cartão que explica. Um tour nunca muda nada.

**Guia**: o lugar na navegação para os tours, o guia do usuário e os atalhos de teclado.

**Guia do usuário**: este guia. O app mostra, e as mesmas páginas estão na documentação do Bardo.

**Página do guia**: uma página do guia do usuário, em inglês e em português.
